use crate::ktp::{decode_frame, encode_frame, FrameType, KtpFrame, KTP_MAX_PAYLOAD_LEN};
use crate::ktp_transport::{
    KtpCryptoDirection, KtpCryptoKey, KtpEncryptedTcpStream, KtpTcpTransportError,
};
use crate::transport::{connect_websocket_request, HeaderPair, TransportError};
use crate::tunnel_async_runtime::TunnelQueueDwellStatsSnapshot;
use crate::tunnel_control::SelectedTunnelRule;
use crate::tunnel_runtime::{NoopTunnelSessionRuntime, TunnelSessionRuntime};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::fmt;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream as TokioTcpStream;
use tokio::runtime::Runtime;
use tokio::time::timeout;
use tungstenite::client::IntoClientRequest;
use tungstenite::http::{HeaderName, HeaderValue};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TunnelDataReadyState {
    pub revision: String,
    pub ingress_rule_ids: Vec<u64>,
    pub egress_rule_ids: Vec<u64>,
    pub failed_rules: Vec<TunnelDataRuleFailure>,
}

impl TunnelDataReadyState {
    pub fn empty(revision: &str) -> Self {
        Self {
            revision: revision.trim().to_string(),
            ingress_rule_ids: Vec::new(),
            egress_rule_ids: Vec::new(),
            failed_rules: Vec::new(),
        }
    }

    pub fn from_selected_rules(revision: &str, rules: &[SelectedTunnelRule]) -> Self {
        let mut ready = Self::empty(revision);
        for rule in rules {
            match rule.role.as_str() {
                "ingress" => ready.ingress_rule_ids.push(rule.id),
                "egress" => ready.egress_rule_ids.push(rule.id),
                "both" => {
                    ready.ingress_rule_ids.push(rule.id);
                    ready.egress_rule_ids.push(rule.id);
                }
                _ => {}
            }
        }
        ready.ingress_rule_ids.sort_unstable();
        ready.ingress_rule_ids.dedup();
        ready.egress_rule_ids.sort_unstable();
        ready.egress_rule_ids.dedup();
        ready
    }
}

#[derive(Clone, Debug)]
pub struct SharedTunnelDataReadyState {
    inner: Arc<Mutex<TunnelDataReadyState>>,
}

impl SharedTunnelDataReadyState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(TunnelDataReadyState::empty(""))),
        }
    }

    pub fn snapshot(&self) -> TunnelDataReadyState {
        self.inner
            .lock()
            .map(|ready| ready.clone())
            .unwrap_or_else(|_| TunnelDataReadyState::empty(""))
    }

    pub fn update_from_selected_rules(&self, revision: &str, rules: &[SelectedTunnelRule]) {
        if let Ok(mut ready) = self.inner.lock() {
            *ready = TunnelDataReadyState::from_selected_rules(revision, rules);
        }
    }
}

impl Default for SharedTunnelDataReadyState {
    fn default() -> Self {
        Self::new()
    }
}

pub trait TunnelDataReadySource {
    fn current_ready(&self) -> TunnelDataReadyState;
}

impl TunnelDataReadySource for TunnelDataReadyState {
    fn current_ready(&self) -> TunnelDataReadyState {
        self.clone()
    }
}

impl TunnelDataReadySource for SharedTunnelDataReadyState {
    fn current_ready(&self) -> TunnelDataReadyState {
        self.snapshot()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TunnelDataRuleFailure {
    pub rule_id: u64,
    pub status: String,
    pub error: String,
}

const RUNTIME_WAIT_ELAPSED_MICROS_BUCKETS: [u64; 17] = [
    10,
    25,
    50,
    100,
    250,
    500,
    1_000,
    2_500,
    5_000,
    10_000,
    25_000,
    50_000,
    100_000,
    250_000,
    500_000,
    1_000_000,
    u64::MAX,
];
const TUNNEL_DATA_FRAME_BATCH_LIMIT: usize = 64;

#[derive(Clone, Debug, Default)]
pub struct SharedTunnelDataDiagnostics {
    inner: Arc<TunnelDataDiagnosticsInner>,
}

#[derive(Debug, Default)]
struct TunnelDataDiagnosticsInner {
    runtime_wait_attempts: AtomicU64,
    runtime_wait_hits: AtomicU64,
    runtime_wait_elapsed_micros_total: AtomicU64,
    runtime_wait_elapsed_micros_max: AtomicU64,
    runtime_wait_elapsed_micros_buckets: [AtomicU64; RUNTIME_WAIT_ELAPSED_MICROS_BUCKETS.len()],
    outbound_runtime_frames: AtomicU64,
    outbound_queue_dwell_frames: AtomicU64,
    outbound_queue_dwell_micros_total: AtomicU64,
    outbound_queue_dwell_micros_max: AtomicU64,
    outbound_queue_dwell_p50_micros: AtomicU64,
    outbound_queue_dwell_p95_micros: AtomicU64,
    outbound_queue_dwell_p99_micros: AtomicU64,
    recent_outbound_queue_dwell_frames: AtomicU64,
    recent_outbound_queue_dwell_micros_total: AtomicU64,
    recent_outbound_queue_dwell_micros_max: AtomicU64,
    recent_outbound_queue_dwell_p50_micros: AtomicU64,
    recent_outbound_queue_dwell_p95_micros: AtomicU64,
    recent_outbound_queue_dwell_p99_micros: AtomicU64,
    socket_idle_reads: AtomicU64,
    socket_idle_empty_reads: AtomicU64,
    socket_read_batches: AtomicU64,
    socket_read_frames: AtomicU64,
    socket_read_max_batch_frames: AtomicU64,
    socket_write_batches: AtomicU64,
    socket_write_frames: AtomicU64,
    socket_write_max_batch_frames: AtomicU64,
    socket_write_batch_limit_max: AtomicU64,
    socket_write_batch_limit_min: AtomicU64,
    socket_write_batch_limit_last: AtomicU64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TunnelDataDiagnosticsSnapshot {
    pub runtime_wait_attempts: u64,
    pub runtime_wait_hits: u64,
    pub runtime_wait_elapsed_micros_total: u64,
    pub runtime_wait_elapsed_micros_max: u64,
    pub runtime_wait_elapsed_p50_micros: u64,
    pub runtime_wait_elapsed_p95_micros: u64,
    pub runtime_wait_elapsed_p99_micros: u64,
    pub outbound_runtime_frames: u64,
    pub outbound_queue_dwell_frames: u64,
    pub outbound_queue_dwell_micros_total: u64,
    pub outbound_queue_dwell_micros_max: u64,
    pub outbound_queue_dwell_p50_micros: u64,
    pub outbound_queue_dwell_p95_micros: u64,
    pub outbound_queue_dwell_p99_micros: u64,
    pub recent_outbound_queue_dwell_frames: u64,
    pub recent_outbound_queue_dwell_micros_total: u64,
    pub recent_outbound_queue_dwell_micros_max: u64,
    pub recent_outbound_queue_dwell_p50_micros: u64,
    pub recent_outbound_queue_dwell_p95_micros: u64,
    pub recent_outbound_queue_dwell_p99_micros: u64,
    pub socket_idle_reads: u64,
    pub socket_idle_empty_reads: u64,
    pub socket_read_batches: u64,
    pub socket_read_frames: u64,
    pub socket_read_max_batch_frames: u64,
    pub socket_write_batches: u64,
    pub socket_write_frames: u64,
    pub socket_write_max_batch_frames: u64,
    pub socket_write_batch_limit_max: u64,
    pub socket_write_batch_limit_min: u64,
    pub socket_write_batch_limit_last: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct TunnelDataWriteBatchStats {
    batches: usize,
    frames: usize,
    max_batch_frames: usize,
    max_batch_limit: usize,
    min_batch_limit: usize,
    last_batch_limit: usize,
}

impl TunnelDataWriteBatchStats {
    fn record_batch_limit(&mut self, max_frames: usize) {
        let max_frames = max_frames.max(1);
        self.max_batch_limit = self.max_batch_limit.max(max_frames);
        self.min_batch_limit = if self.min_batch_limit == 0 {
            max_frames
        } else {
            self.min_batch_limit.min(max_frames)
        };
        self.last_batch_limit = max_frames;
    }

    fn record_batch(&mut self, frame_count: usize) {
        if frame_count == 0 {
            return;
        }
        self.batches += 1;
        self.frames += frame_count;
        self.max_batch_frames = self.max_batch_frames.max(frame_count);
    }

    fn merge(&mut self, other: Self) {
        self.batches += other.batches;
        self.frames += other.frames;
        self.max_batch_frames = self.max_batch_frames.max(other.max_batch_frames);
        self.max_batch_limit = self.max_batch_limit.max(other.max_batch_limit);
        self.min_batch_limit = match (self.min_batch_limit, other.min_batch_limit) {
            (0, value) => value,
            (value, 0) => value,
            (left, right) => left.min(right),
        };
        if other.last_batch_limit > 0 {
            self.last_batch_limit = other.last_batch_limit;
        }
    }
}

impl SharedTunnelDataDiagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> TunnelDataDiagnosticsSnapshot {
        let runtime_wait_attempts = self.inner.runtime_wait_attempts.load(Ordering::Relaxed);
        let mut runtime_wait_elapsed_micros_buckets =
            [0u64; RUNTIME_WAIT_ELAPSED_MICROS_BUCKETS.len()];
        for (index, bucket) in self
            .inner
            .runtime_wait_elapsed_micros_buckets
            .iter()
            .enumerate()
        {
            runtime_wait_elapsed_micros_buckets[index] = bucket.load(Ordering::Relaxed);
        }
        TunnelDataDiagnosticsSnapshot {
            runtime_wait_attempts,
            runtime_wait_hits: self.inner.runtime_wait_hits.load(Ordering::Relaxed),
            runtime_wait_elapsed_micros_total: self
                .inner
                .runtime_wait_elapsed_micros_total
                .load(Ordering::Relaxed),
            runtime_wait_elapsed_micros_max: self
                .inner
                .runtime_wait_elapsed_micros_max
                .load(Ordering::Relaxed),
            runtime_wait_elapsed_p50_micros: runtime_wait_elapsed_percentile(
                &runtime_wait_elapsed_micros_buckets,
                runtime_wait_attempts,
                50,
            ),
            runtime_wait_elapsed_p95_micros: runtime_wait_elapsed_percentile(
                &runtime_wait_elapsed_micros_buckets,
                runtime_wait_attempts,
                95,
            ),
            runtime_wait_elapsed_p99_micros: runtime_wait_elapsed_percentile(
                &runtime_wait_elapsed_micros_buckets,
                runtime_wait_attempts,
                99,
            ),
            outbound_runtime_frames: self.inner.outbound_runtime_frames.load(Ordering::Relaxed),
            outbound_queue_dwell_frames: self
                .inner
                .outbound_queue_dwell_frames
                .load(Ordering::Relaxed),
            outbound_queue_dwell_micros_total: self
                .inner
                .outbound_queue_dwell_micros_total
                .load(Ordering::Relaxed),
            outbound_queue_dwell_micros_max: self
                .inner
                .outbound_queue_dwell_micros_max
                .load(Ordering::Relaxed),
            outbound_queue_dwell_p50_micros: self
                .inner
                .outbound_queue_dwell_p50_micros
                .load(Ordering::Relaxed),
            outbound_queue_dwell_p95_micros: self
                .inner
                .outbound_queue_dwell_p95_micros
                .load(Ordering::Relaxed),
            outbound_queue_dwell_p99_micros: self
                .inner
                .outbound_queue_dwell_p99_micros
                .load(Ordering::Relaxed),
            recent_outbound_queue_dwell_frames: self
                .inner
                .recent_outbound_queue_dwell_frames
                .load(Ordering::Relaxed),
            recent_outbound_queue_dwell_micros_total: self
                .inner
                .recent_outbound_queue_dwell_micros_total
                .load(Ordering::Relaxed),
            recent_outbound_queue_dwell_micros_max: self
                .inner
                .recent_outbound_queue_dwell_micros_max
                .load(Ordering::Relaxed),
            recent_outbound_queue_dwell_p50_micros: self
                .inner
                .recent_outbound_queue_dwell_p50_micros
                .load(Ordering::Relaxed),
            recent_outbound_queue_dwell_p95_micros: self
                .inner
                .recent_outbound_queue_dwell_p95_micros
                .load(Ordering::Relaxed),
            recent_outbound_queue_dwell_p99_micros: self
                .inner
                .recent_outbound_queue_dwell_p99_micros
                .load(Ordering::Relaxed),
            socket_idle_reads: self.inner.socket_idle_reads.load(Ordering::Relaxed),
            socket_idle_empty_reads: self.inner.socket_idle_empty_reads.load(Ordering::Relaxed),
            socket_read_batches: self.inner.socket_read_batches.load(Ordering::Relaxed),
            socket_read_frames: self.inner.socket_read_frames.load(Ordering::Relaxed),
            socket_read_max_batch_frames: self
                .inner
                .socket_read_max_batch_frames
                .load(Ordering::Relaxed),
            socket_write_batches: self.inner.socket_write_batches.load(Ordering::Relaxed),
            socket_write_frames: self.inner.socket_write_frames.load(Ordering::Relaxed),
            socket_write_max_batch_frames: self
                .inner
                .socket_write_max_batch_frames
                .load(Ordering::Relaxed),
            socket_write_batch_limit_max: self
                .inner
                .socket_write_batch_limit_max
                .load(Ordering::Relaxed),
            socket_write_batch_limit_min: self
                .inner
                .socket_write_batch_limit_min
                .load(Ordering::Relaxed),
            socket_write_batch_limit_last: self
                .inner
                .socket_write_batch_limit_last
                .load(Ordering::Relaxed),
        }
    }

    fn record_outbound_runtime_frames(&self, count: usize) {
        self.inner
            .outbound_runtime_frames
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    fn record_runtime_wait(&self, elapsed: Duration, frame_count: usize) {
        let elapsed_micros = elapsed.as_micros().min(u128::from(u64::MAX)) as u64;
        self.inner
            .runtime_wait_attempts
            .fetch_add(1, Ordering::Relaxed);
        if frame_count > 0 {
            self.inner.runtime_wait_hits.fetch_add(1, Ordering::Relaxed);
        }
        self.inner
            .runtime_wait_elapsed_micros_total
            .fetch_add(elapsed_micros, Ordering::Relaxed);
        update_atomic_max(&self.inner.runtime_wait_elapsed_micros_max, elapsed_micros);
        self.inner.runtime_wait_elapsed_micros_buckets
            [runtime_wait_elapsed_bucket_index(elapsed_micros)]
        .fetch_add(1, Ordering::Relaxed);
    }

    fn record_outbound_queue_dwell_snapshot(
        &self,
        snapshot: Option<TunnelQueueDwellStatsSnapshot>,
    ) {
        let Some(snapshot) = snapshot else {
            return;
        };
        self.inner
            .outbound_queue_dwell_frames
            .store(snapshot.frames, Ordering::Relaxed);
        self.inner
            .outbound_queue_dwell_micros_total
            .store(snapshot.micros_total, Ordering::Relaxed);
        self.inner
            .outbound_queue_dwell_micros_max
            .store(snapshot.micros_max, Ordering::Relaxed);
        self.inner
            .outbound_queue_dwell_p50_micros
            .store(snapshot.p50_micros, Ordering::Relaxed);
        self.inner
            .outbound_queue_dwell_p95_micros
            .store(snapshot.p95_micros, Ordering::Relaxed);
        self.inner
            .outbound_queue_dwell_p99_micros
            .store(snapshot.p99_micros, Ordering::Relaxed);
    }

    fn record_recent_outbound_queue_dwell_snapshot(
        &self,
        snapshot: Option<TunnelQueueDwellStatsSnapshot>,
    ) {
        let Some(snapshot) = snapshot else {
            return;
        };
        self.inner
            .recent_outbound_queue_dwell_frames
            .store(snapshot.frames, Ordering::Relaxed);
        self.inner
            .recent_outbound_queue_dwell_micros_total
            .store(snapshot.micros_total, Ordering::Relaxed);
        self.inner
            .recent_outbound_queue_dwell_micros_max
            .store(snapshot.micros_max, Ordering::Relaxed);
        self.inner
            .recent_outbound_queue_dwell_p50_micros
            .store(snapshot.p50_micros, Ordering::Relaxed);
        self.inner
            .recent_outbound_queue_dwell_p95_micros
            .store(snapshot.p95_micros, Ordering::Relaxed);
        self.inner
            .recent_outbound_queue_dwell_p99_micros
            .store(snapshot.p99_micros, Ordering::Relaxed);
    }

    fn record_socket_idle_read(&self) {
        self.inner.socket_idle_reads.fetch_add(1, Ordering::Relaxed);
    }

    fn record_socket_idle_empty_read(&self) {
        self.inner
            .socket_idle_empty_reads
            .fetch_add(1, Ordering::Relaxed);
    }

    fn record_socket_read_batch(&self, frame_count: usize) {
        if frame_count == 0 {
            return;
        }
        self.inner
            .socket_read_batches
            .fetch_add(1, Ordering::Relaxed);
        self.inner
            .socket_read_frames
            .fetch_add(frame_count as u64, Ordering::Relaxed);
        update_atomic_max(&self.inner.socket_read_max_batch_frames, frame_count as u64);
    }

    fn record_socket_write_batches(&self, stats: TunnelDataWriteBatchStats) {
        update_atomic_max(
            &self.inner.socket_write_batch_limit_max,
            stats.max_batch_limit as u64,
        );
        update_atomic_min_nonzero(
            &self.inner.socket_write_batch_limit_min,
            stats.min_batch_limit as u64,
        );
        if stats.last_batch_limit > 0 {
            self.inner
                .socket_write_batch_limit_last
                .store(stats.last_batch_limit as u64, Ordering::Relaxed);
        }
        if stats.frames == 0 || stats.batches == 0 {
            return;
        }
        self.inner
            .socket_write_batches
            .fetch_add(stats.batches as u64, Ordering::Relaxed);
        self.inner
            .socket_write_frames
            .fetch_add(stats.frames as u64, Ordering::Relaxed);
        update_atomic_max(
            &self.inner.socket_write_max_batch_frames,
            stats.max_batch_frames as u64,
        );
    }
}

impl TunnelDataDiagnosticsSnapshot {
    pub fn has_activity(&self) -> bool {
        self.runtime_wait_attempts > 0
            || self.runtime_wait_hits > 0
            || self.outbound_runtime_frames > 0
            || self.outbound_queue_dwell_frames > 0
            || self.recent_outbound_queue_dwell_frames > 0
            || self.socket_idle_reads > 0
            || self.socket_idle_empty_reads > 0
            || self.socket_read_batches > 0
            || self.socket_read_frames > 0
            || self.socket_write_batches > 0
            || self.socket_write_frames > 0
    }
}

pub fn tunnel_data_diagnostics_line(snapshot: &TunnelDataDiagnosticsSnapshot) -> String {
    format!(
        "tunnel data diagnostics: runtime_wait_attempts={} runtime_wait_hits={} runtime_wait_elapsed_micros_total={} runtime_wait_elapsed_micros_max={} runtime_wait_elapsed_p50_micros={} runtime_wait_elapsed_p95_micros={} runtime_wait_elapsed_p99_micros={} outbound_runtime_frames={} outbound_queue_dwell_frames={} outbound_queue_dwell_micros_total={} outbound_queue_dwell_micros_max={} outbound_queue_dwell_p50_micros={} outbound_queue_dwell_p95_micros={} outbound_queue_dwell_p99_micros={} recent_outbound_queue_dwell_frames={} recent_outbound_queue_dwell_micros_total={} recent_outbound_queue_dwell_micros_max={} recent_outbound_queue_dwell_p50_micros={} recent_outbound_queue_dwell_p95_micros={} recent_outbound_queue_dwell_p99_micros={} socket_idle_reads={} socket_idle_empty_reads={} socket_read_batches={} socket_read_frames={} socket_read_max_batch_frames={} socket_write_batches={} socket_write_frames={} socket_write_max_batch_frames={} socket_write_batch_limit_max={} socket_write_batch_limit_min={} socket_write_batch_limit_last={}",
        snapshot.runtime_wait_attempts,
        snapshot.runtime_wait_hits,
        snapshot.runtime_wait_elapsed_micros_total,
        snapshot.runtime_wait_elapsed_micros_max,
        snapshot.runtime_wait_elapsed_p50_micros,
        snapshot.runtime_wait_elapsed_p95_micros,
        snapshot.runtime_wait_elapsed_p99_micros,
        snapshot.outbound_runtime_frames,
        snapshot.outbound_queue_dwell_frames,
        snapshot.outbound_queue_dwell_micros_total,
        snapshot.outbound_queue_dwell_micros_max,
        snapshot.outbound_queue_dwell_p50_micros,
        snapshot.outbound_queue_dwell_p95_micros,
        snapshot.outbound_queue_dwell_p99_micros,
        snapshot.recent_outbound_queue_dwell_frames,
        snapshot.recent_outbound_queue_dwell_micros_total,
        snapshot.recent_outbound_queue_dwell_micros_max,
        snapshot.recent_outbound_queue_dwell_p50_micros,
        snapshot.recent_outbound_queue_dwell_p95_micros,
        snapshot.recent_outbound_queue_dwell_p99_micros,
        snapshot.socket_idle_reads,
        snapshot.socket_idle_empty_reads,
        snapshot.socket_read_batches,
        snapshot.socket_read_frames,
        snapshot.socket_read_max_batch_frames,
        snapshot.socket_write_batches,
        snapshot.socket_write_frames,
        snapshot.socket_write_max_batch_frames,
        snapshot.socket_write_batch_limit_max,
        snapshot.socket_write_batch_limit_min,
        snapshot.socket_write_batch_limit_last
    )
}

fn runtime_wait_elapsed_bucket_index(elapsed_micros: u64) -> usize {
    RUNTIME_WAIT_ELAPSED_MICROS_BUCKETS
        .iter()
        .position(|bucket| elapsed_micros <= *bucket)
        .unwrap_or(RUNTIME_WAIT_ELAPSED_MICROS_BUCKETS.len() - 1)
}

fn runtime_wait_elapsed_percentile(
    buckets: &[u64; RUNTIME_WAIT_ELAPSED_MICROS_BUCKETS.len()],
    total: u64,
    percentile: u64,
) -> u64 {
    if total == 0 {
        return 0;
    }
    let rank = ((total * percentile).saturating_add(99) / 100).max(1);
    let mut cumulative = 0u64;
    for (index, count) in buckets.iter().enumerate() {
        cumulative = cumulative.saturating_add(*count);
        if cumulative >= rank {
            return RUNTIME_WAIT_ELAPSED_MICROS_BUCKETS[index];
        }
    }
    RUNTIME_WAIT_ELAPSED_MICROS_BUCKETS[RUNTIME_WAIT_ELAPSED_MICROS_BUCKETS.len() - 1]
}

fn update_atomic_max(target: &AtomicU64, candidate: u64) {
    let mut current = target.load(Ordering::Relaxed);
    while candidate > current {
        match target.compare_exchange_weak(current, candidate, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => break,
            Err(next) => current = next,
        }
    }
}

fn update_atomic_min_nonzero(target: &AtomicU64, candidate: u64) {
    if candidate == 0 {
        return;
    }
    let mut current = target.load(Ordering::Relaxed);
    loop {
        let next = if current == 0 {
            candidate
        } else {
            current.min(candidate)
        };
        if next == current {
            break;
        }
        match target.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(actual) => current = actual,
        }
    }
}

pub trait TunnelDataSocket {
    fn send_frame(&mut self, frame: &[u8]) -> Result<(), TransportError>;
    fn send_ktp_frame(&mut self, frame: &KtpFrame) -> Result<(), TransportError> {
        let bytes = encode_frame(frame)
            .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
        self.send_frame(&bytes)
    }
    fn send_ktp_frame_batch(&mut self, frames: &[KtpFrame]) -> Result<(), TransportError> {
        for frame in frames {
            self.send_ktp_frame(frame)?;
        }
        Ok(())
    }
    fn read_frame(&mut self) -> Result<Vec<u8>, TransportError>;
    fn read_optional_frame(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        self.read_frame().map(Some)
    }
    fn read_optional_frame_with_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<Vec<u8>>, TransportError> {
        let _ = timeout;
        self.read_optional_frame()
    }
    fn read_optional_ktp_frame_batch(
        &mut self,
        max_frames: usize,
    ) -> Result<Option<Vec<KtpFrame>>, TransportError> {
        let _ = max_frames;
        self.read_optional_frame()?
            .map(|bytes| {
                decode_frame(&bytes, KTP_MAX_PAYLOAD_LEN)
                    .map(|frame| vec![frame])
                    .map_err(|error| TransportError::RequestFailed(error.to_string()))
            })
            .transpose()
    }
    fn read_optional_ktp_frame_batch_with_timeout(
        &mut self,
        timeout: Duration,
        max_frames: usize,
    ) -> Result<Option<Vec<KtpFrame>>, TransportError> {
        let _ = max_frames;
        self.read_optional_frame_with_timeout(timeout)?
            .map(|bytes| {
                decode_frame(&bytes, KTP_MAX_PAYLOAD_LEN)
                    .map(|frame| vec![frame])
                    .map_err(|error| TransportError::RequestFailed(error.to_string()))
            })
            .transpose()
    }
}

pub trait TunnelDataTransport {
    type Socket: TunnelDataSocket;

    fn connect_tunnel_data(
        &mut self,
        url: &str,
        headers: &[HeaderPair],
    ) -> Result<Self::Socket, TransportError>;
}

#[derive(Debug, Clone)]
pub struct KtpEncryptedTcpTunnelDataTransport {
    auth: KtpEncryptedTcpTunnelDataAuth,
    read_timeout: Duration,
    max_payload_len: usize,
    max_buffer_len: usize,
}

#[derive(Debug, Clone)]
enum KtpEncryptedTcpTunnelDataAuth {
    StaticKey(KtpCryptoKey),
    Token {
        token: String,
        version: KtpTcpAuthVersion,
    },
}

impl KtpEncryptedTcpTunnelDataTransport {
    pub fn new(key: KtpCryptoKey) -> Self {
        Self {
            auth: KtpEncryptedTcpTunnelDataAuth::StaticKey(key),
            read_timeout: Duration::from_secs(2),
            max_payload_len: KTP_MAX_PAYLOAD_LEN,
            max_buffer_len: 1024 * 1024,
        }
    }

    pub fn new_with_token(token: &str) -> Self {
        Self::new_with_token_auth_version(token, KtpTcpAuthVersion::V1)
    }

    pub fn new_with_token_auth_version(token: &str, version: KtpTcpAuthVersion) -> Self {
        Self {
            auth: KtpEncryptedTcpTunnelDataAuth::Token {
                token: token.trim().to_string(),
                version,
            },
            read_timeout: Duration::from_secs(2),
            max_payload_len: KTP_MAX_PAYLOAD_LEN,
            max_buffer_len: 1024 * 1024,
        }
    }

    pub fn with_read_timeout(mut self, timeout: Duration) -> Self {
        self.read_timeout = timeout;
        self
    }
}

impl TunnelDataTransport for KtpEncryptedTcpTunnelDataTransport {
    type Socket = KtpEncryptedTcpTunnelDataSocket;

    fn connect_tunnel_data(
        &mut self,
        url: &str,
        _headers: &[HeaderPair],
    ) -> Result<Self::Socket, TransportError> {
        let address = parse_ktp_tcp_tunnel_data_address(url)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|error| ktp_tcp_request_failed("ktp_tcp_runtime_init_failed", error))?;
        let stream = runtime
            .block_on(TokioTcpStream::connect(&address))
            .map_err(|error| ktp_tcp_request_failed("ktp_tcp_connect_failed", error))?;
        let (stream, key) = match &self.auth {
            KtpEncryptedTcpTunnelDataAuth::StaticKey(key) => (stream, key.clone()),
            KtpEncryptedTcpTunnelDataAuth::Token { token, version } => {
                let nonce = random_ktp_tcp_auth_nonce()?;
                let preface = build_ktp_tcp_auth_preface_with_version(token, nonce, *version)?;
                let mut stream = stream;
                runtime
                    .block_on(stream.write_all(&preface))
                    .map_err(|error| ktp_tcp_request_failed("ktp_tcp_auth_write_failed", error))?;
                (
                    stream,
                    derive_ktp_tcp_crypto_key_with_version(token, nonce, *version),
                )
            }
        };
        let stream = KtpEncryptedTcpStream::from_stream(
            stream,
            key,
            KtpCryptoDirection::ClientToRelay,
            KtpCryptoDirection::RelayToClient,
            self.max_payload_len,
            self.max_buffer_len,
        );
        Ok(KtpEncryptedTcpTunnelDataSocket {
            runtime,
            stream,
            read_timeout: self.read_timeout,
            max_payload_len: self.max_payload_len,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KtpTcpAuthVersion {
    V1,
    V2,
}

impl KtpTcpAuthVersion {
    fn magic(self) -> &'static [u8; 4] {
        match self {
            Self::V1 => b"KTA1",
            Self::V2 => b"KTA2",
        }
    }
}

pub fn build_ktp_tcp_auth_preface(token: &str, nonce: [u8; 16]) -> Result<Vec<u8>, TransportError> {
    build_ktp_tcp_auth_preface_with_version(token, nonce, KtpTcpAuthVersion::V1)
}

pub fn build_ktp_tcp_auth_preface_with_version(
    token: &str,
    nonce: [u8; 16],
    version: KtpTcpAuthVersion,
) -> Result<Vec<u8>, TransportError> {
    let token = token.trim();
    if token.is_empty() {
        return Err(TransportError::EmptyToken);
    }

    let fingerprint = Sha256::digest(token.as_bytes());
    let mut preface = Vec::with_capacity(84);
    preface.extend_from_slice(version.magic());
    preface.extend_from_slice(&nonce);
    preface.extend_from_slice(&fingerprint);
    let tag = ktp_tcp_auth_tag(token, nonce, &fingerprint, version)?;
    preface.extend_from_slice(&tag);
    Ok(preface)
}

pub fn derive_ktp_tcp_crypto_key(token: &str, nonce: [u8; 16]) -> KtpCryptoKey {
    derive_ktp_tcp_crypto_key_with_version(token, nonce, KtpTcpAuthVersion::V1)
}

pub fn derive_ktp_tcp_crypto_key_with_version(
    token: &str,
    nonce: [u8; 16],
    version: KtpTcpAuthVersion,
) -> KtpCryptoKey {
    match version {
        KtpTcpAuthVersion::V1 => derive_ktp_tcp_crypto_key_v1(token, nonce),
        KtpTcpAuthVersion::V2 => derive_ktp_tcp_crypto_key_v2(token, nonce),
    }
}

fn derive_ktp_tcp_crypto_key_v1(token: &str, nonce: [u8; 16]) -> KtpCryptoKey {
    let mut hash = Sha256::new();
    hash.update(b"kelicloud ktp tcp data v1");
    hash.update(token.trim().as_bytes());
    hash.update(nonce);
    let digest = hash.finalize();
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&digest);
    KtpCryptoKey::from_bytes(bytes)
}

fn derive_ktp_tcp_crypto_key_v2(token: &str, nonce: [u8; 16]) -> KtpCryptoKey {
    let prk = ktp_tcp_auth_v2_prk(token, nonce);
    KtpCryptoKey::from_bytes(hmac_sha256(
        &prk,
        &[b"kelicloud ktp tcp data v2 key", &[1u8]],
    ))
}

fn ktp_tcp_auth_tag(
    token: &str,
    nonce: [u8; 16],
    fingerprint: &[u8],
    version: KtpTcpAuthVersion,
) -> Result<[u8; 32], TransportError> {
    match version {
        KtpTcpAuthVersion::V1 => {
            let mut mac = Hmac::<Sha256>::new_from_slice(token.as_bytes())
                .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
            mac.update(b"kelicloud ktp tcp auth v1");
            mac.update(&nonce);
            mac.update(fingerprint);
            let mut tag = [0u8; 32];
            tag.copy_from_slice(&mac.finalize().into_bytes());
            Ok(tag)
        }
        KtpTcpAuthVersion::V2 => {
            let prk = ktp_tcp_auth_v2_prk(token, nonce);
            Ok(hmac_sha256(
                &prk,
                &[b"kelicloud ktp tcp auth v2", &nonce, fingerprint],
            ))
        }
    }
}

fn ktp_tcp_auth_v2_prk(token: &str, nonce: [u8; 16]) -> [u8; 32] {
    hmac_sha256(&nonce, &[token.trim().as_bytes()])
}

fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    for part in parts {
        mac.update(part);
    }
    let mut output = [0u8; 32];
    output.copy_from_slice(&mac.finalize().into_bytes());
    output
}

fn random_ktp_tcp_auth_nonce() -> Result<[u8; 16], TransportError> {
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce)
        .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
    Ok(nonce)
}

pub struct KtpEncryptedTcpTunnelDataSocket {
    runtime: Runtime,
    stream: KtpEncryptedTcpStream,
    read_timeout: Duration,
    max_payload_len: usize,
}

impl TunnelDataSocket for KtpEncryptedTcpTunnelDataSocket {
    fn send_frame(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        let frame = decode_frame(frame, self.max_payload_len)
            .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
        self.runtime
            .block_on(self.stream.send_frame(&frame))
            .map_err(ktp_tcp_transport_error_to_transport)
    }

    fn send_ktp_frame(&mut self, frame: &KtpFrame) -> Result<(), TransportError> {
        self.runtime
            .block_on(self.stream.send_frame(frame))
            .map_err(ktp_tcp_transport_error_to_transport)
    }

    fn send_ktp_frame_batch(&mut self, frames: &[KtpFrame]) -> Result<(), TransportError> {
        self.runtime
            .block_on(self.stream.send_frames(frames))
            .map_err(ktp_tcp_transport_error_to_transport)
    }

    fn read_frame(&mut self) -> Result<Vec<u8>, TransportError> {
        let frame = self
            .runtime
            .block_on(self.stream.next_frame())
            .map_err(ktp_tcp_transport_error_to_transport)?;
        encode_frame(&frame).map_err(|error| TransportError::RequestFailed(error.to_string()))
    }

    fn read_optional_frame(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        self.read_optional_frame_with_timeout(self.read_timeout)
    }

    fn read_optional_frame_with_timeout(
        &mut self,
        timeout_duration: Duration,
    ) -> Result<Option<Vec<u8>>, TransportError> {
        let result = self
            .runtime
            .block_on(async { timeout(timeout_duration, self.stream.next_frame()).await });
        match result {
            Ok(Ok(frame)) => encode_frame(&frame)
                .map(Some)
                .map_err(|error| TransportError::RequestFailed(error.to_string())),
            Ok(Err(error)) => Err(ktp_tcp_transport_error_to_transport(error)),
            Err(_) => Ok(None),
        }
    }

    fn read_optional_ktp_frame_batch(
        &mut self,
        max_frames: usize,
    ) -> Result<Option<Vec<KtpFrame>>, TransportError> {
        self.read_optional_ktp_frame_batch_with_timeout(self.read_timeout, max_frames)
    }

    fn read_optional_ktp_frame_batch_with_timeout(
        &mut self,
        timeout_duration: Duration,
        max_frames: usize,
    ) -> Result<Option<Vec<KtpFrame>>, TransportError> {
        let result = self.runtime.block_on(async {
            timeout(timeout_duration, self.stream.next_frames(max_frames.max(1))).await
        });
        match result {
            Ok(Ok(frames)) => Ok(Some(frames)),
            Ok(Err(error)) => Err(ktp_tcp_transport_error_to_transport(error)),
            Err(_) => Ok(None),
        }
    }
}

fn parse_ktp_tcp_tunnel_data_address(url: &str) -> Result<String, TransportError> {
    let trimmed = url.trim();
    let Some(rest) = trimmed
        .strip_prefix("ktp+tcp://")
        .or_else(|| trimmed.strip_prefix("tcp://"))
    else {
        return Err(TransportError::UnsupportedScheme(trimmed.to_string()));
    };
    let address = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if address.is_empty() {
        return Err(TransportError::EmptyEndpoint);
    }
    Ok(address)
}

fn ktp_tcp_transport_error_to_transport(error: KtpTcpTransportError) -> TransportError {
    match error {
        KtpTcpTransportError::Closed => TransportError::SocketClosed,
        other => ktp_tcp_request_failed(other.code(), other),
    }
}

fn ktp_tcp_request_failed(code: &'static str, detail: impl fmt::Display) -> TransportError {
    TransportError::RequestFailed(format!("ktp_tcp_error code={code} detail={detail}"))
}

#[derive(Debug, Default, Clone)]
pub struct TungsteniteTunnelDataTransport {
    custom_dns: String,
}

impl TungsteniteTunnelDataTransport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn new_with_custom_dns(custom_dns: &str) -> Self {
        Self {
            custom_dns: custom_dns.trim().to_string(),
        }
    }
}

impl TunnelDataTransport for TungsteniteTunnelDataTransport {
    type Socket = TungsteniteTunnelDataSocket;

    fn connect_tunnel_data(
        &mut self,
        url: &str,
        headers: &[HeaderPair],
    ) -> Result<Self::Socket, TransportError> {
        let mut request = url
            .into_client_request()
            .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
        for (name, value) in headers {
            let header_name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
            let header_value = HeaderValue::from_str(value)
                .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
            request.headers_mut().insert(header_name, header_value);
        }

        let (socket, _response) = connect_websocket_request(request, &self.custom_dns)?;
        Ok(TungsteniteTunnelDataSocket {
            socket,
            read_timeout: Duration::from_secs(2),
        })
    }
}

pub struct TungsteniteTunnelDataSocket {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    read_timeout: Duration,
}

impl TunnelDataSocket for TungsteniteTunnelDataSocket {
    fn send_frame(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        self.socket
            .send(Message::Binary(frame.to_vec().into()))
            .map_err(|error| TransportError::RequestFailed(error.to_string()))
    }

    fn read_frame(&mut self) -> Result<Vec<u8>, TransportError> {
        self.read_next_frame(false, self.read_timeout)?
            .ok_or_else(|| TransportError::RequestFailed("tunnel data frame timeout".to_string()))
    }

    fn read_optional_frame(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        self.read_optional_frame_with_timeout(self.read_timeout)
    }

    fn read_optional_frame_with_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<Vec<u8>>, TransportError> {
        self.read_next_frame(true, timeout)
    }
}

impl TungsteniteTunnelDataSocket {
    fn read_next_frame(
        &mut self,
        timeout_as_idle: bool,
        read_timeout: Duration,
    ) -> Result<Option<Vec<u8>>, TransportError> {
        self.set_read_timeout(Some(read_timeout))?;
        loop {
            match self.socket.read() {
                Ok(Message::Binary(bytes)) => return Ok(Some(bytes.to_vec())),
                Ok(Message::Text(text)) => return Ok(Some(text.to_string().into_bytes())),
                Ok(Message::Close(_)) => return Err(TransportError::SocketClosed),
                Ok(_) => continue,
                Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::Interrupted => {
                    continue;
                }
                Err(tungstenite::Error::Io(error)) if is_idle_read_error(error.kind()) => {
                    if timeout_as_idle {
                        return Ok(None);
                    }
                    return Err(TransportError::RequestFailed(
                        "tunnel data frame timeout".to_string(),
                    ));
                }
                Err(error) => return Err(TransportError::RequestFailed(error.to_string())),
            }
        }
    }

    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> Result<(), TransportError> {
        match self.socket.get_mut() {
            MaybeTlsStream::Plain(stream) => stream.set_read_timeout(timeout),
            MaybeTlsStream::Rustls(stream) => stream.sock.set_read_timeout(timeout),
            _ => Ok(()),
        }
        .map_err(|error| TransportError::RequestFailed(error.to_string()))
    }
}

fn is_idle_read_error(kind: ErrorKind) -> bool {
    matches!(kind, ErrorKind::WouldBlock | ErrorKind::TimedOut)
}

#[cfg(test)]
mod tests {
    use super::{is_idle_read_error, SharedTunnelDataDiagnostics};
    use std::io::ErrorKind;
    use std::time::Duration;

    #[test]
    fn data_read_timeout_errors_are_idle_but_interrupted_retries() {
        assert!(is_idle_read_error(ErrorKind::TimedOut));
        assert!(is_idle_read_error(ErrorKind::WouldBlock));
        assert!(!is_idle_read_error(ErrorKind::Interrupted));
        assert!(!is_idle_read_error(ErrorKind::ConnectionReset));
    }

    #[test]
    fn diagnostics_snapshot_reports_runtime_wait_latency_percentiles() {
        let diagnostics = SharedTunnelDataDiagnostics::new();
        diagnostics.record_runtime_wait(Duration::from_micros(8), 1);
        diagnostics.record_runtime_wait(Duration::from_micros(90), 1);
        diagnostics.record_runtime_wait(Duration::from_micros(900), 1);
        diagnostics.record_runtime_wait(Duration::from_micros(9_000), 1);

        let snapshot = diagnostics.snapshot();

        assert_eq!(snapshot.runtime_wait_elapsed_p50_micros, 100);
        assert_eq!(snapshot.runtime_wait_elapsed_p95_micros, 10_000);
        assert_eq!(snapshot.runtime_wait_elapsed_p99_micros, 10_000);
    }
}

pub fn run_tunnel_data_once<T>(
    url: &str,
    headers: &[HeaderPair],
    agent_id_hint: &str,
    agent_version: &str,
    ready: &TunnelDataReadyState,
    transport: &mut T,
) -> Result<(), TransportError>
where
    T: TunnelDataTransport,
{
    let mut socket = match transport.connect_tunnel_data(url, headers) {
        Ok(socket) => socket,
        Err(error) if is_nonfatal_connect_error(&error) => return Ok(()),
        Err(error) => return Err(error),
    };

    let hello_payload = encode_hello_payload(agent_id_hint, agent_version, &ready.revision)?;
    let hello_frame = KtpFrame::connection(FrameType::Hello, hello_payload);
    if send_tunnel_data_ktp_frame(&mut socket, &hello_frame)? == SendFrameOutcome::Closed {
        return Ok(());
    }

    if read_tunnel_data_hello_ack(&mut socket)? == ReadFrameOutcome::Closed {
        return Ok(());
    }

    let ready_payload = encode_ready_payload(ready)?;
    let ready_frame = KtpFrame::connection(FrameType::Ready, ready_payload);
    if send_tunnel_data_ktp_frame(&mut socket, &ready_frame)? == SendFrameOutcome::Closed {
        return Ok(());
    }

    Ok(())
}

pub fn run_tunnel_data_session<T>(
    url: &str,
    headers: &[HeaderPair],
    agent_id_hint: &str,
    agent_version: &str,
    ready: &TunnelDataReadyState,
    transport: &mut T,
) -> Result<(), TransportError>
where
    T: TunnelDataTransport,
{
    run_tunnel_data_session_with_ready_source(
        url,
        headers,
        agent_id_hint,
        agent_version,
        ready,
        transport,
    )
}

pub fn run_tunnel_data_session_with_ready_source<T, S>(
    url: &str,
    headers: &[HeaderPair],
    agent_id_hint: &str,
    agent_version: &str,
    ready_source: &S,
    transport: &mut T,
) -> Result<(), TransportError>
where
    T: TunnelDataTransport,
    S: TunnelDataReadySource,
{
    let mut runtime = NoopTunnelSessionRuntime;
    run_tunnel_data_session_with_ready_source_and_runtime(
        url,
        headers,
        agent_id_hint,
        agent_version,
        ready_source,
        transport,
        &mut runtime,
    )
}

pub fn run_tunnel_data_session_with_ready_source_and_runtime<T, S, R>(
    url: &str,
    headers: &[HeaderPair],
    agent_id_hint: &str,
    agent_version: &str,
    ready_source: &S,
    transport: &mut T,
    runtime: &mut R,
) -> Result<(), TransportError>
where
    T: TunnelDataTransport,
    S: TunnelDataReadySource,
    R: TunnelSessionRuntime,
{
    let diagnostics = SharedTunnelDataDiagnostics::new();
    run_tunnel_data_session_with_ready_source_runtime_and_diagnostics(
        url,
        headers,
        agent_id_hint,
        agent_version,
        ready_source,
        transport,
        runtime,
        &diagnostics,
    )
}

pub fn run_tunnel_data_session_with_ready_source_runtime_and_diagnostics<T, S, R>(
    url: &str,
    headers: &[HeaderPair],
    agent_id_hint: &str,
    agent_version: &str,
    ready_source: &S,
    transport: &mut T,
    runtime: &mut R,
    diagnostics: &SharedTunnelDataDiagnostics,
) -> Result<(), TransportError>
where
    T: TunnelDataTransport,
    S: TunnelDataReadySource,
    R: TunnelSessionRuntime,
{
    run_tunnel_data_session_with_ready_source_runtime_diagnostics_and_reporter(
        url,
        headers,
        agent_id_hint,
        agent_version,
        ready_source,
        transport,
        runtime,
        diagnostics,
        Duration::MAX,
        |_| {},
    )
}

pub fn run_tunnel_data_session_with_ready_source_runtime_diagnostics_and_reporter<T, S, R, F>(
    url: &str,
    headers: &[HeaderPair],
    agent_id_hint: &str,
    agent_version: &str,
    ready_source: &S,
    transport: &mut T,
    runtime: &mut R,
    diagnostics: &SharedTunnelDataDiagnostics,
    diagnostics_report_interval: Duration,
    report_diagnostics: F,
) -> Result<(), TransportError>
where
    T: TunnelDataTransport,
    S: TunnelDataReadySource,
    R: TunnelSessionRuntime,
    F: FnMut(&TunnelDataDiagnosticsSnapshot),
{
    let mut socket = match transport.connect_tunnel_data(url, headers) {
        Ok(socket) => socket,
        Err(error) if is_nonfatal_connect_error(&error) => return Ok(()),
        Err(error) => return Err(error),
    };

    let result =
        run_connected_tunnel_data_session_with_ready_source_runtime_diagnostics_and_reporter(
            &mut socket,
            agent_id_hint,
            agent_version,
            ready_source,
            runtime,
            diagnostics,
            diagnostics_report_interval,
            report_diagnostics,
        );
    let cleanup_result = runtime.close_all_sessions("carrier_disconnect");
    match (result, cleanup_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Ok(()), Err(error)) => Err(error),
        (Err(error), Ok(())) | (Err(error), Err(_)) => Err(error),
    }
}

fn run_connected_tunnel_data_session_with_ready_source_runtime_diagnostics_and_reporter<
    S,
    R,
    U,
    F,
>(
    socket: &mut U,
    agent_id_hint: &str,
    agent_version: &str,
    ready_source: &S,
    runtime: &mut R,
    diagnostics: &SharedTunnelDataDiagnostics,
    diagnostics_report_interval: Duration,
    mut report_diagnostics: F,
) -> Result<(), TransportError>
where
    S: TunnelDataReadySource,
    R: TunnelSessionRuntime,
    U: TunnelDataSocket,
    F: FnMut(&TunnelDataDiagnosticsSnapshot),
{
    let mut last_diagnostics_report = Instant::now();

    let hello_ready = ready_source.current_ready();
    let hello_payload = encode_hello_payload(agent_id_hint, agent_version, &hello_ready.revision)?;
    let hello_frame = KtpFrame::connection(FrameType::Hello, hello_payload);
    if send_tunnel_data_ktp_frame(socket, &hello_frame)? == SendFrameOutcome::Closed {
        return Ok(());
    }

    if read_tunnel_data_hello_ack(socket)? == ReadFrameOutcome::Closed {
        return Ok(());
    }

    runtime.tick()?;
    let mut last_ready = ready_source.current_ready();
    if send_ready_frame(socket, &last_ready)? == SendFrameOutcome::Closed {
        return Ok(());
    }

    loop {
        runtime.tick()?;
        let current_ready = ready_source.current_ready();
        if current_ready != last_ready {
            if send_ready_frame(socket, &current_ready)? == SendFrameOutcome::Closed {
                return Ok(());
            }
            last_ready = current_ready;
        }
        let sent_runtime_frames = drain_tunnel_session_runtime_frames(socket, runtime)?;
        diagnostics.record_outbound_runtime_frames(sent_runtime_frames.frames);
        diagnostics.record_socket_write_batches(sent_runtime_frames);
        diagnostics.record_outbound_queue_dwell_snapshot(runtime.outbound_queue_dwell_snapshot());
        diagnostics.record_recent_outbound_queue_dwell_snapshot(
            runtime.recent_outbound_queue_dwell_snapshot(),
        );
        if sent_runtime_frames.frames == 0 {
            if let Some(timeout) = runtime.tunnel_data_client_frame_wait_timeout() {
                let wait_started = Instant::now();
                let waited_runtime_frames =
                    drain_tunnel_session_runtime_frames_after_wait(socket, runtime, timeout)?;
                diagnostics
                    .record_runtime_wait(wait_started.elapsed(), waited_runtime_frames.frames);
                diagnostics.record_outbound_runtime_frames(waited_runtime_frames.frames);
                diagnostics.record_socket_write_batches(waited_runtime_frames);
                diagnostics
                    .record_outbound_queue_dwell_snapshot(runtime.outbound_queue_dwell_snapshot());
                diagnostics.record_recent_outbound_queue_dwell_snapshot(
                    runtime.recent_outbound_queue_dwell_snapshot(),
                );
                if waited_runtime_frames.frames > 0 {
                    report_tunnel_data_diagnostics_if_due(
                        diagnostics,
                        &mut last_diagnostics_report,
                        diagnostics_report_interval,
                        &mut report_diagnostics,
                    );
                }
            }
        }
        report_tunnel_data_diagnostics_if_due(
            diagnostics,
            &mut last_diagnostics_report,
            diagnostics_report_interval,
            &mut report_diagnostics,
        );
        diagnostics.record_socket_idle_read();
        let read_result = match runtime.tunnel_data_socket_idle_timeout() {
            Some(timeout) => socket
                .read_optional_ktp_frame_batch_with_timeout(timeout, TUNNEL_DATA_FRAME_BATCH_LIMIT),
            None => socket.read_optional_ktp_frame_batch(TUNNEL_DATA_FRAME_BATCH_LIMIT),
        };
        match read_result {
            Ok(Some(frames)) => {
                diagnostics.record_socket_read_batch(frames.len());
                handle_tunnel_data_session_frames(socket, frames, runtime)?
            }
            Ok(None) => {
                diagnostics.record_socket_idle_empty_read();
                continue;
            }
            Err(TransportError::SocketClosed) => return Ok(()),
            Err(error) => return Err(error),
        }
    }
}

fn report_tunnel_data_diagnostics_if_due<F>(
    diagnostics: &SharedTunnelDataDiagnostics,
    last_report: &mut Instant,
    interval: Duration,
    report: &mut F,
) where
    F: FnMut(&TunnelDataDiagnosticsSnapshot),
{
    if interval == Duration::MAX || last_report.elapsed() < interval {
        return;
    }
    let snapshot = diagnostics.snapshot();
    if snapshot.has_activity() {
        report(&snapshot);
        *last_report = Instant::now();
    }
}

fn send_ready_frame<S>(
    socket: &mut S,
    ready: &TunnelDataReadyState,
) -> Result<SendFrameOutcome, TransportError>
where
    S: TunnelDataSocket,
{
    let ready_payload = encode_ready_payload(ready)?;
    let ready_frame = KtpFrame::connection(FrameType::Ready, ready_payload);
    send_tunnel_data_ktp_frame(socket, &ready_frame)
}

pub fn tunnel_data_startup_line(url: &str, enabled: bool) -> String {
    if !enabled {
        return "tunnel data: disabled".to_string();
    }

    format!(
        "tunnel data: enabled url={}{}",
        redact_token_in_url(url),
        tunnel_data_startup_transport_fields(url)
    )
}

fn tunnel_data_startup_transport_fields(url: &str) -> &'static str {
    let url = url.trim();
    if url.starts_with("ktp+tcp://") || url.starts_with("tcp://") {
        " carrier=ktp_tcp crypto=ktp_aead auth=ktp_token_preface_v1"
    } else {
        ""
    }
}

pub fn tunnel_data_reconnect_delay_after_attempt(
    current_delay: Duration,
    session_succeeded: bool,
) -> (Duration, Duration) {
    let current_delay = if current_delay.is_zero() {
        Duration::from_secs(5)
    } else {
        current_delay
    };
    let sleep_delay = if session_succeeded {
        Duration::from_secs(15)
    } else {
        current_delay
    };
    let next_delay = sleep_delay
        .checked_add(sleep_delay)
        .unwrap_or(Duration::MAX)
        .min(Duration::from_secs(60));
    (sleep_delay, next_delay)
}

fn is_nonfatal_connect_error(error: &TransportError) -> bool {
    match error {
        TransportError::RequestFailed(message) => {
            let message = message.to_ascii_lowercase();
            message.contains("404")
                || message.contains("403")
                || message.contains("feature_disabled")
        }
        TransportError::SocketClosed => true,
        TransportError::InvalidClientToken { .. }
        | TransportError::EmptyEndpoint
        | TransportError::EmptyToken
        | TransportError::UnsupportedScheme(_) => false,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SendFrameOutcome {
    Sent,
    Closed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReadFrameOutcome {
    Frame,
    Closed,
}

fn send_tunnel_data_ktp_frame<S>(
    socket: &mut S,
    frame: &KtpFrame,
) -> Result<SendFrameOutcome, TransportError>
where
    S: TunnelDataSocket,
{
    match socket.send_ktp_frame(frame) {
        Ok(()) => Ok(SendFrameOutcome::Sent),
        Err(TransportError::SocketClosed) => Ok(SendFrameOutcome::Closed),
        Err(error) => Err(error),
    }
}

fn send_tunnel_data_ktp_frame_batch<S>(
    socket: &mut S,
    frames: &[KtpFrame],
) -> Result<SendFrameOutcome, TransportError>
where
    S: TunnelDataSocket,
{
    match socket.send_ktp_frame_batch(frames) {
        Ok(()) => Ok(SendFrameOutcome::Sent),
        Err(TransportError::SocketClosed) => Ok(SendFrameOutcome::Closed),
        Err(error) => Err(error),
    }
}

fn read_tunnel_data_hello_ack<S>(socket: &mut S) -> Result<ReadFrameOutcome, TransportError>
where
    S: TunnelDataSocket,
{
    let bytes = match socket.read_frame() {
        Ok(bytes) => bytes,
        Err(TransportError::SocketClosed) => return Ok(ReadFrameOutcome::Closed),
        Err(error) => return Err(error),
    };
    let frame = decode_frame(&bytes, KTP_MAX_PAYLOAD_LEN)
        .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
    if frame.frame_type != FrameType::HelloAck {
        return Err(TransportError::RequestFailed(format!(
            "expected tunnel data hello_ack, got {:?}",
            frame.frame_type
        )));
    }
    Ok(ReadFrameOutcome::Frame)
}

fn drain_tunnel_session_runtime_frames<S, R>(
    socket: &mut S,
    runtime: &mut R,
) -> Result<TunnelDataWriteBatchStats, TransportError>
where
    S: TunnelDataSocket,
    R: TunnelSessionRuntime,
{
    let mut stats = TunnelDataWriteBatchStats::default();
    let max_frames = tunnel_session_runtime_frame_batch_limit(runtime);
    stats.record_batch_limit(max_frames);
    loop {
        let frames = runtime.next_client_frames(max_frames)?;
        if frames.is_empty() {
            return Ok(stats);
        }
        let frame_count = send_tunnel_session_runtime_frame_batch(socket, frames)?;
        stats.record_batch(frame_count);
    }
}

fn drain_tunnel_session_runtime_frames_after_wait<S, R>(
    socket: &mut S,
    runtime: &mut R,
    timeout: Duration,
) -> Result<TunnelDataWriteBatchStats, TransportError>
where
    S: TunnelDataSocket,
    R: TunnelSessionRuntime,
{
    let max_frames = tunnel_session_runtime_frame_batch_limit(runtime);
    let mut stats = TunnelDataWriteBatchStats::default();
    stats.record_batch_limit(max_frames);
    let frames = runtime.next_client_frames_after_wait(max_frames, timeout)?;
    if frames.is_empty() {
        return Ok(stats);
    }
    let frame_count = send_tunnel_session_runtime_frame_batch(socket, frames)?;
    stats.record_batch(frame_count);
    stats.merge(drain_tunnel_session_runtime_frames(socket, runtime)?);
    Ok(stats)
}

fn tunnel_session_runtime_frame_batch_limit<R>(runtime: &R) -> usize
where
    R: TunnelSessionRuntime,
{
    runtime
        .client_frame_batch_limit(TUNNEL_DATA_FRAME_BATCH_LIMIT)
        .clamp(1, TUNNEL_DATA_FRAME_BATCH_LIMIT)
}

fn send_tunnel_session_runtime_frame_batch<S>(
    socket: &mut S,
    frames: Vec<KtpFrame>,
) -> Result<usize, TransportError>
where
    S: TunnelDataSocket,
{
    if frames.is_empty() {
        return Ok(0);
    }
    let frame_count = frames.len();
    let _ = send_tunnel_data_ktp_frame_batch(socket, &frames)?;
    Ok(frame_count)
}

fn handle_tunnel_data_session_frame<S, R>(
    socket: &mut S,
    frame: KtpFrame,
    runtime: &mut R,
) -> Result<(), TransportError>
where
    S: TunnelDataSocket,
    R: TunnelSessionRuntime,
{
    match frame.frame_type {
        FrameType::Ping => {
            let pong = KtpFrame::connection(FrameType::Pong, frame.payload);
            let _ = send_tunnel_data_ktp_frame(socket, &pong)?;
        }
        FrameType::SessionOpen
        | FrameType::SessionAccept
        | FrameType::SessionData
        | FrameType::SessionWindow
        | FrameType::SessionClose
        | FrameType::SessionError => {
            for response in runtime.handle_server_frame(frame)? {
                let _ = send_tunnel_data_ktp_frame(socket, &response)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_tunnel_data_session_frames<S, R>(
    socket: &mut S,
    frames: Vec<KtpFrame>,
    runtime: &mut R,
) -> Result<(), TransportError>
where
    S: TunnelDataSocket,
    R: TunnelSessionRuntime,
{
    for frame in frames {
        handle_tunnel_data_session_frame(socket, frame, runtime)?;
    }
    Ok(())
}

fn encode_hello_payload(
    agent_id_hint: &str,
    agent_version: &str,
    revision: &str,
) -> Result<Vec<u8>, TransportError> {
    let mut payload = Vec::new();
    write_string(&mut payload, agent_id_hint)?;
    write_string(&mut payload, agent_version)?;
    write_string(&mut payload, revision)?;
    write_string_list(&mut payload, &["tcp", "multiplex", "flow_control", "stats"])?;
    Ok(payload)
}

fn encode_ready_payload(ready: &TunnelDataReadyState) -> Result<Vec<u8>, TransportError> {
    let mut payload = Vec::new();
    write_string(&mut payload, &ready.revision)?;
    write_u64_list(&mut payload, &ready.ingress_rule_ids)?;
    write_u64_list(&mut payload, &ready.egress_rule_ids)?;
    write_count(&mut payload, ready.failed_rules.len(), "failed rule count")?;
    for failure in &ready.failed_rules {
        payload.extend_from_slice(&failure.rule_id.to_be_bytes());
        write_string(&mut payload, &failure.status)?;
        write_string(&mut payload, &failure.error)?;
    }
    Ok(payload)
}

fn write_string(output: &mut Vec<u8>, value: &str) -> Result<(), TransportError> {
    let bytes = value.as_bytes();
    write_count(output, bytes.len(), "string length")?;
    output.extend_from_slice(bytes);
    Ok(())
}

fn write_string_list(output: &mut Vec<u8>, values: &[&str]) -> Result<(), TransportError> {
    write_count(output, values.len(), "string list count")?;
    for value in values {
        write_string(output, value)?;
    }
    Ok(())
}

fn write_u64_list(output: &mut Vec<u8>, values: &[u64]) -> Result<(), TransportError> {
    write_count(output, values.len(), "u64 list count")?;
    for value in values {
        output.extend_from_slice(&value.to_be_bytes());
    }
    Ok(())
}

fn write_count(output: &mut Vec<u8>, len: usize, field: &str) -> Result<(), TransportError> {
    let len = u16::try_from(len)
        .map_err(|_| TransportError::RequestFailed(format!("{field} too long: exceeds u16")))?;
    output.extend_from_slice(&len.to_be_bytes());
    Ok(())
}

fn redact_token_in_url(url: &str) -> String {
    let Some((base, query)) = url.split_once('?') else {
        return url.to_string();
    };

    let redacted_query = query
        .split('&')
        .map(|part| {
            part.split_once('=')
                .filter(|(key, _)| *key == "token")
                .map(|(key, _)| format!("{key}=redacted"))
                .unwrap_or_else(|| part.to_string())
        })
        .collect::<Vec<_>>()
        .join("&");

    format!("{base}?{redacted_query}")
}
