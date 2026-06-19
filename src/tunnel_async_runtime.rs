use crate::ktp::{FrameLeg, FrameType, KtpFrame};
use crate::tunnel_session::{
    encode_session_accept_payload, encode_session_open_payload, TunnelSessionOpenPayload,
};
use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TunnelRuntimeLimits {
    pub max_sessions_per_agent: usize,
    pub max_outbound_frames: usize,
    pub max_session_pending_bytes: usize,
    pub tcp_read_chunk_size: usize,
    pub target_dial_timeout: Duration,
    pub idle_timeout: Duration,
    pub relay_batch_policy: TunnelRelayBatchPolicy,
    pub relay_batch_tuning: TunnelRelayBatchTuning,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TunnelRelayBatchPolicy {
    #[default]
    Fixed,
    Adaptive,
}

impl TunnelRelayBatchPolicy {
    pub fn parse_config_value(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "fixed" => Some(Self::Fixed),
            "adaptive" => Some(Self::Adaptive),
            _ => None,
        }
    }

    pub fn config_value(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Adaptive => "adaptive",
        }
    }

    pub fn effective_batch_frames(
        self,
        configured_batch_frames: usize,
        active_sessions: usize,
    ) -> usize {
        self.effective_batch_frames_with_dwell(
            configured_batch_frames,
            active_sessions,
            TunnelQueueDwellStatsSnapshot::default(),
        )
    }

    pub fn effective_batch_frames_with_dwell(
        self,
        configured_batch_frames: usize,
        active_sessions: usize,
        outbound_queue_dwell: TunnelQueueDwellStatsSnapshot,
    ) -> usize {
        self.effective_batch_frames_with_tuning(
            configured_batch_frames,
            active_sessions,
            outbound_queue_dwell,
            TunnelRelayBatchTuning::default(),
        )
    }

    pub fn effective_batch_frames_with_tuning(
        self,
        configured_batch_frames: usize,
        active_sessions: usize,
        outbound_queue_dwell: TunnelQueueDwellStatsSnapshot,
        tuning: TunnelRelayBatchTuning,
    ) -> usize {
        let configured_batch_frames = configured_batch_frames.max(1);
        match self {
            Self::Fixed => configured_batch_frames,
            Self::Adaptive if outbound_queue_dwell.p95_micros >= tuning.severe_dwell_p95_micros => {
                configured_batch_frames.min(tuning.severe_batch_cap.max(1))
            }
            Self::Adaptive
                if outbound_queue_dwell.p95_micros >= tuning.elevated_dwell_p95_micros =>
            {
                configured_batch_frames.min(tuning.elevated_batch_cap.max(1))
            }
            Self::Adaptive if active_sessions >= tuning.high_session_threshold.max(1) => {
                configured_batch_frames.min(tuning.elevated_batch_cap.max(1))
            }
            Self::Adaptive => configured_batch_frames,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TunnelRelayBatchTuning {
    pub high_session_threshold: usize,
    pub elevated_dwell_p95_micros: u64,
    pub severe_dwell_p95_micros: u64,
    pub elevated_batch_cap: usize,
    pub severe_batch_cap: usize,
}

impl Default for TunnelRelayBatchTuning {
    fn default() -> Self {
        Self {
            high_session_threshold: 8,
            elevated_dwell_p95_micros: 50_000,
            severe_dwell_p95_micros: 250_000,
            elevated_batch_cap: 16,
            severe_batch_cap: 8,
        }
    }
}

impl Default for TunnelRuntimeLimits {
    fn default() -> Self {
        Self {
            max_sessions_per_agent: 1024,
            max_outbound_frames: 4096,
            max_session_pending_bytes: 4 * 1024 * 1024,
            tcp_read_chunk_size: 16 * 1024,
            target_dial_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(600),
            relay_batch_policy: TunnelRelayBatchPolicy::Fixed,
            relay_batch_tuning: TunnelRelayBatchTuning::default(),
        }
    }
}

const OUTBOUND_QUEUE_DWELL_MICROS_BUCKETS: [u64; 17] = [
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
const OUTBOUND_QUEUE_DWELL_RECENT_SAMPLES: usize = 16;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TunnelRuntimeError {
    code: &'static str,
    message: String,
}

impl TunnelRuntimeError {
    pub fn backpressure_limit() -> Self {
        Self {
            code: "backpressure_limit",
            message: "tunnel outbound frame queue is full".to_string(),
        }
    }

    pub fn runtime_unavailable(message: impl Into<String>) -> Self {
        Self {
            code: "runtime_unavailable",
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for TunnelRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.message.trim().is_empty() {
            write!(f, "{}", self.code)
        } else {
            write!(f, "{}: {}", self.code, self.message)
        }
    }
}

impl Error for TunnelRuntimeError {}

#[derive(Clone, Debug)]
pub struct AsyncTunnelFrameQueue {
    inner: Arc<AsyncTunnelFrameQueueInner>,
    capacity: usize,
}

#[derive(Debug)]
struct AsyncTunnelFrameQueueInner {
    frames: Mutex<VecDeque<QueuedKtpFrame>>,
    ready: Condvar,
    shared_ready: Option<Arc<TunnelFrameReadyNotifier>>,
    stats: TunnelRuntimeStats,
}

#[derive(Debug)]
struct QueuedKtpFrame {
    frame: KtpFrame,
    enqueued_at: Instant,
}

#[derive(Debug, Default)]
pub struct TunnelFrameReadyNotifier {
    generation: Mutex<u64>,
    ready: Condvar,
}

impl TunnelFrameReadyNotifier {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn generation(&self) -> u64 {
        self.generation
            .lock()
            .map(|generation| *generation)
            .unwrap_or(0)
    }

    pub fn notify(&self) {
        let Ok(mut generation) = self.generation.lock() else {
            return;
        };
        *generation = generation.saturating_add(1);
        self.ready.notify_all();
    }

    pub fn wait_for_change(&self, observed_generation: u64, timeout: Duration) -> u64 {
        let Ok(generation) = self.generation.lock() else {
            return observed_generation;
        };
        if *generation != observed_generation || timeout.is_zero() {
            return *generation;
        }
        let Ok((generation, _)) =
            self.ready
                .wait_timeout_while(generation, timeout, |generation| {
                    *generation == observed_generation
                })
        else {
            return observed_generation;
        };
        *generation
    }
}

impl AsyncTunnelFrameQueue {
    pub fn new(capacity: usize) -> Self {
        Self::new_internal(capacity, None, TunnelRuntimeStats::default())
    }

    pub fn new_with_notifier(capacity: usize, notifier: Arc<TunnelFrameReadyNotifier>) -> Self {
        Self::new_internal(capacity, Some(notifier), TunnelRuntimeStats::default())
    }

    pub fn new_with_stats(capacity: usize, stats: TunnelRuntimeStats) -> Self {
        Self::new_internal(capacity, None, stats)
    }

    fn new_internal(
        capacity: usize,
        shared_ready: Option<Arc<TunnelFrameReadyNotifier>>,
        stats: TunnelRuntimeStats,
    ) -> Self {
        Self {
            inner: Arc::new(AsyncTunnelFrameQueueInner {
                frames: Mutex::new(VecDeque::new()),
                ready: Condvar::new(),
                shared_ready,
                stats,
            }),
            capacity,
        }
    }

    pub fn try_push(&self, frame: KtpFrame) -> Result<(), TunnelRuntimeError> {
        {
            let mut inner = self.inner.frames.lock().map_err(|_| {
                TunnelRuntimeError::runtime_unavailable("frame queue is unavailable")
            })?;
            if inner.len() >= self.capacity {
                return Err(TunnelRuntimeError::backpressure_limit());
            }
            inner.push_back(QueuedKtpFrame {
                frame,
                enqueued_at: Instant::now(),
            });
        }
        self.inner.ready.notify_one();
        if let Some(shared_ready) = &self.inner.shared_ready {
            shared_ready.notify();
        }
        Ok(())
    }

    pub fn pop(&self) -> Option<KtpFrame> {
        self.inner
            .frames
            .lock()
            .ok()
            .and_then(|mut inner| inner.pop_front())
            .map(|queued| self.into_frame_with_dwell(queued))
    }

    pub fn drain(&self, max_frames: usize) -> Vec<KtpFrame> {
        if max_frames == 0 {
            return Vec::new();
        }
        let queued_frames = {
            let Ok(mut inner) = self.inner.frames.lock() else {
                return Vec::new();
            };
            let count = max_frames.min(inner.len());
            inner.drain(..count).collect::<Vec<_>>()
        };
        queued_frames
            .into_iter()
            .map(|queued| self.into_frame_with_dwell(queued))
            .collect()
    }

    pub fn drain_after_wait(&self, max_frames: usize, timeout: Duration) -> Vec<KtpFrame> {
        if max_frames == 0 {
            return Vec::new();
        }
        let queued_frames = {
            let Ok(mut inner) = self.inner.frames.lock() else {
                return Vec::new();
            };
            if inner.is_empty() && !timeout.is_zero() {
                let Ok((waited_inner, _)) =
                    self.inner
                        .ready
                        .wait_timeout_while(inner, timeout, |inner| inner.is_empty())
                else {
                    return Vec::new();
                };
                inner = waited_inner;
            }
            let count = max_frames.min(inner.len());
            inner.drain(..count).collect::<Vec<_>>()
        };
        queued_frames
            .into_iter()
            .map(|queued| self.into_frame_with_dwell(queued))
            .collect()
    }

    pub fn clear(&self) -> usize {
        self.inner
            .frames
            .lock()
            .map(|mut inner| {
                let count = inner.len();
                inner.clear();
                count
            })
            .unwrap_or(0)
    }

    pub fn remove_session_frames(&self, session_id: u64) -> usize {
        self.inner
            .frames
            .lock()
            .map(|mut inner| {
                let before = inner.len();
                inner.retain(|queued| queued.frame.session_id != session_id);
                before.saturating_sub(inner.len())
            })
            .unwrap_or(0)
    }

    pub fn len(&self) -> usize {
        self.inner
            .frames
            .lock()
            .map(|inner| inner.len())
            .unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn into_frame_with_dwell(&self, queued: QueuedKtpFrame) -> KtpFrame {
        self.inner
            .stats
            .record_outbound_queue_dwell(queued.enqueued_at.elapsed());
        queued.frame
    }
}

#[derive(Clone, Debug, Default)]
pub struct TunnelRuntimeStats {
    active_sessions: Arc<AtomicUsize>,
    total_sessions: Arc<AtomicU64>,
    bytes_in: Arc<AtomicU64>,
    bytes_out: Arc<AtomicU64>,
    rule_session_counts: Arc<Mutex<HashMap<u64, usize>>>,
    outbound_queue_dwell: Arc<TunnelQueueDwellStatsInner>,
    recent_outbound_queue_dwell: Arc<TunnelQueueDwellRecentStatsInner>,
}

#[derive(Debug, Default)]
struct TunnelQueueDwellStatsInner {
    frames: AtomicU64,
    micros_total: AtomicU64,
    micros_max: AtomicU64,
    micros_buckets: [AtomicU64; OUTBOUND_QUEUE_DWELL_MICROS_BUCKETS.len()],
}

#[derive(Debug, Default)]
struct TunnelQueueDwellRecentStatsInner {
    samples: Mutex<VecDeque<u64>>,
}

impl TunnelQueueDwellRecentStatsInner {
    fn record(&self, elapsed_micros: u64) {
        let Ok(mut samples) = self.samples.lock() else {
            return;
        };
        if samples.len() >= OUTBOUND_QUEUE_DWELL_RECENT_SAMPLES {
            samples.pop_front();
        }
        samples.push_back(elapsed_micros);
    }

    fn snapshot(&self) -> TunnelQueueDwellStatsSnapshot {
        let Ok(samples) = self.samples.lock() else {
            return TunnelQueueDwellStatsSnapshot::default();
        };
        let mut buckets = [0u64; OUTBOUND_QUEUE_DWELL_MICROS_BUCKETS.len()];
        let mut total = 0u64;
        let mut max = 0u64;
        for elapsed_micros in samples.iter().copied() {
            total = total.saturating_add(elapsed_micros);
            max = max.max(elapsed_micros);
            buckets[outbound_queue_dwell_bucket_index(elapsed_micros)] += 1;
        }
        outbound_queue_dwell_snapshot(samples.len() as u64, total, max, &buckets)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TunnelQueueDwellStatsSnapshot {
    pub frames: u64,
    pub micros_total: u64,
    pub micros_max: u64,
    pub p50_micros: u64,
    pub p95_micros: u64,
    pub p99_micros: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TunnelRuntimeStatsSnapshot {
    pub active_sessions: usize,
    pub total_sessions: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub rule_session_counts: HashMap<u64, usize>,
    pub outbound_queue_dwell: TunnelQueueDwellStatsSnapshot,
    pub recent_outbound_queue_dwell: TunnelQueueDwellStatsSnapshot,
}

impl TunnelRuntimeStats {
    pub fn session_opened(&self, rule_id: u64) {
        self.active_sessions.fetch_add(1, Ordering::Relaxed);
        self.total_sessions.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut counts) = self.rule_session_counts.lock() {
            *counts.entry(rule_id).or_default() += 1;
        }
    }

    pub fn session_closed(&self, rule_id: u64) {
        let _ = self
            .active_sessions
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                Some(value.saturating_sub(1))
            });
        if let Ok(mut counts) = self.rule_session_counts.lock() {
            let entry = counts.entry(rule_id).or_default();
            *entry = entry.saturating_sub(1);
        }
    }

    pub fn bytes_in(&self, _rule_id: u64, bytes: u64) {
        self.bytes_in.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn bytes_out(&self, _rule_id: u64, bytes: u64) {
        self.bytes_out.fetch_add(bytes, Ordering::Relaxed);
    }

    fn record_outbound_queue_dwell(&self, elapsed: Duration) {
        let elapsed_micros = elapsed.as_micros().min(u128::from(u64::MAX)) as u64;
        self.outbound_queue_dwell
            .frames
            .fetch_add(1, Ordering::Relaxed);
        self.outbound_queue_dwell
            .micros_total
            .fetch_add(elapsed_micros, Ordering::Relaxed);
        update_atomic_max(&self.outbound_queue_dwell.micros_max, elapsed_micros);
        self.outbound_queue_dwell.micros_buckets[outbound_queue_dwell_bucket_index(elapsed_micros)]
            .fetch_add(1, Ordering::Relaxed);
        self.recent_outbound_queue_dwell.record(elapsed_micros);
    }

    pub fn snapshot(&self) -> TunnelRuntimeStatsSnapshot {
        let outbound_queue_dwell_frames = self.outbound_queue_dwell.frames.load(Ordering::Relaxed);
        let outbound_queue_dwell_micros_max =
            self.outbound_queue_dwell.micros_max.load(Ordering::Relaxed);
        let mut outbound_queue_dwell_buckets = [0u64; OUTBOUND_QUEUE_DWELL_MICROS_BUCKETS.len()];
        for (index, bucket) in self.outbound_queue_dwell.micros_buckets.iter().enumerate() {
            outbound_queue_dwell_buckets[index] = bucket.load(Ordering::Relaxed);
        }
        TunnelRuntimeStatsSnapshot {
            active_sessions: self.active_sessions.load(Ordering::Relaxed),
            total_sessions: self.total_sessions.load(Ordering::Relaxed),
            bytes_in: self.bytes_in.load(Ordering::Relaxed),
            bytes_out: self.bytes_out.load(Ordering::Relaxed),
            rule_session_counts: self
                .rule_session_counts
                .lock()
                .map(|counts| counts.clone())
                .unwrap_or_default(),
            outbound_queue_dwell: outbound_queue_dwell_snapshot(
                outbound_queue_dwell_frames,
                self.outbound_queue_dwell
                    .micros_total
                    .load(Ordering::Relaxed),
                outbound_queue_dwell_micros_max,
                &outbound_queue_dwell_buckets,
            ),
            recent_outbound_queue_dwell: self.recent_outbound_queue_dwell.snapshot(),
        }
    }
}

fn outbound_queue_dwell_snapshot(
    frames: u64,
    micros_total: u64,
    micros_max: u64,
    buckets: &[u64; OUTBOUND_QUEUE_DWELL_MICROS_BUCKETS.len()],
) -> TunnelQueueDwellStatsSnapshot {
    TunnelQueueDwellStatsSnapshot {
        frames,
        micros_total,
        micros_max,
        p50_micros: outbound_queue_dwell_percentile(buckets, frames, 50, micros_max),
        p95_micros: outbound_queue_dwell_percentile(buckets, frames, 95, micros_max),
        p99_micros: outbound_queue_dwell_percentile(buckets, frames, 99, micros_max),
    }
}

fn outbound_queue_dwell_bucket_index(elapsed_micros: u64) -> usize {
    OUTBOUND_QUEUE_DWELL_MICROS_BUCKETS
        .iter()
        .position(|bucket| elapsed_micros <= *bucket)
        .unwrap_or(OUTBOUND_QUEUE_DWELL_MICROS_BUCKETS.len() - 1)
}

fn outbound_queue_dwell_percentile(
    buckets: &[u64; OUTBOUND_QUEUE_DWELL_MICROS_BUCKETS.len()],
    total: u64,
    percentile: u64,
    overflow_bucket_value: u64,
) -> u64 {
    if total == 0 {
        return 0;
    }
    let rank = ((total * percentile).saturating_add(99) / 100).max(1);
    let mut cumulative = 0u64;
    for (index, count) in buckets.iter().enumerate() {
        cumulative = cumulative.saturating_add(*count);
        if cumulative >= rank {
            if index == OUTBOUND_QUEUE_DWELL_MICROS_BUCKETS.len() - 1 {
                return overflow_bucket_value;
            }
            return OUTBOUND_QUEUE_DWELL_MICROS_BUCKETS[index];
        }
    }
    overflow_bucket_value
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outbound_queue_dwell_percentile_uses_observed_max_for_overflow_bucket() {
        let mut buckets = [0u64; OUTBOUND_QUEUE_DWELL_MICROS_BUCKETS.len()];
        buckets[OUTBOUND_QUEUE_DWELL_MICROS_BUCKETS.len() - 1] = 1;

        assert_eq!(
            outbound_queue_dwell_percentile(&buckets, 1, 50, 1_100_000),
            1_100_000
        );
        assert_eq!(
            outbound_queue_dwell_percentile(&buckets, 1, 95, 1_100_000),
            1_100_000
        );
        assert_eq!(
            outbound_queue_dwell_percentile(&buckets, 1, 99, 1_100_000),
            1_100_000
        );
    }

    #[test]
    fn outbound_queue_dwell_percentile_keeps_finite_bucket_upper_bounds() {
        let mut buckets = [0u64; OUTBOUND_QUEUE_DWELL_MICROS_BUCKETS.len()];
        buckets[8] = 1;

        assert_eq!(
            outbound_queue_dwell_percentile(&buckets, 1, 95, 90_000_000),
            OUTBOUND_QUEUE_DWELL_MICROS_BUCKETS[8]
        );
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TunnelIngressListenerSpec {
    pub rule_id: u64,
    pub listen_address: String,
    pub listen_port: u16,
    pub source_allowlist: String,
}

#[derive(Clone)]
pub struct AsyncTunnelCore {
    limits: TunnelRuntimeLimits,
    outbound: AsyncTunnelFrameQueue,
    sessions: Arc<Mutex<HashMap<u64, AsyncTunnelSession>>>,
    listeners: Arc<Mutex<HashMap<u64, tokio::task::JoinHandle<()>>>>,
    next_session_id: Arc<AtomicU64>,
    stats: TunnelRuntimeStats,
}

#[derive(Clone)]
struct AsyncTunnelSession {
    rule_id: u64,
    to_tcp: mpsc::Sender<Vec<u8>>,
    pending_bytes: Arc<AtomicUsize>,
    tasks: Arc<Mutex<AsyncTunnelSessionTasks>>,
}

#[derive(Debug, Default)]
struct AsyncTunnelSessionTasks {
    reader: Option<tokio::task::JoinHandle<()>>,
}

impl AsyncTunnelCore {
    pub fn new(limits: TunnelRuntimeLimits) -> Self {
        Self::new_internal(limits, None)
    }

    pub fn new_with_frame_ready_notifier(
        limits: TunnelRuntimeLimits,
        notifier: Arc<TunnelFrameReadyNotifier>,
    ) -> Self {
        Self::new_internal(limits, Some(notifier))
    }

    fn new_internal(
        limits: TunnelRuntimeLimits,
        frame_ready_notifier: Option<Arc<TunnelFrameReadyNotifier>>,
    ) -> Self {
        let stats = TunnelRuntimeStats::default();
        let outbound = match frame_ready_notifier {
            Some(notifier) => AsyncTunnelFrameQueue::new_internal(
                limits.max_outbound_frames,
                Some(notifier),
                stats.clone(),
            ),
            None => {
                AsyncTunnelFrameQueue::new_with_stats(limits.max_outbound_frames, stats.clone())
            }
        };
        Self {
            outbound,
            limits,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            listeners: Arc::new(Mutex::new(HashMap::new())),
            next_session_id: Arc::new(AtomicU64::new(1)),
            stats,
        }
    }

    pub async fn start_ingress_listener(
        &self,
        spec: TunnelIngressListenerSpec,
    ) -> Result<(), TunnelRuntimeError> {
        let endpoint = format!("{}:{}", spec.listen_address.trim(), spec.listen_port);
        let listener = TcpListener::bind(&endpoint)
            .await
            .map_err(|error| TunnelRuntimeError::listen_bind_failed(error.to_string()))?;
        let core = self.clone();
        let rule_id = spec.rule_id;
        let handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer)) => {
                        if !crate::tunnel_runtime::source_addr_allowed(
                            &peer.to_string(),
                            &spec.source_allowlist,
                        ) {
                            continue;
                        }
                        let session_id = core.next_session_id.fetch_add(1, Ordering::Relaxed);
                        let _ = core
                            .attach_ingress_stream(
                                session_id,
                                rule_id,
                                spec.listen_address.clone(),
                                spec.listen_port,
                                stream,
                                peer.to_string(),
                            )
                            .await;
                    }
                    Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
                }
            }
        });

        let mut listeners = self
            .listeners
            .lock()
            .map_err(|_| TunnelRuntimeError::runtime_unavailable("listener map is unavailable"))?;
        if let Some(previous) = listeners.insert(rule_id, handle) {
            previous.abort();
        }
        Ok(())
    }

    pub async fn stop_ingress_listener(&self, rule_id: u64) -> Result<(), TunnelRuntimeError> {
        let handle = self
            .listeners
            .lock()
            .map_err(|_| TunnelRuntimeError::runtime_unavailable("listener map is unavailable"))?
            .remove(&rule_id);
        if let Some(handle) = handle {
            handle.abort();
        }
        Ok(())
    }

    pub async fn open_egress_session(
        &self,
        session_id: u64,
        rule_id: u64,
        target_host: &str,
        target_port: u16,
        _open_payload: Vec<u8>,
    ) -> Result<Vec<KtpFrame>, TunnelRuntimeError> {
        self.ensure_agent_session_capacity()?;
        let target = format!("{}:{}", target_host.trim(), target_port);
        let stream =
            tokio::time::timeout(self.limits.target_dial_timeout, TcpStream::connect(&target))
                .await
                .map_err(|_| TunnelRuntimeError::target_connect_failed("target dial timed out"))?
                .map_err(|error| TunnelRuntimeError::target_connect_failed(error.to_string()))?;
        let (reader, writer) = stream.into_split();
        let (tx, rx) = mpsc::channel::<Vec<u8>>(64);
        let pending_bytes = Arc::new(AtomicUsize::new(0));

        let session = AsyncTunnelSession::new(rule_id, tx, Arc::clone(&pending_bytes));
        self.sessions
            .lock()
            .map_err(|_| TunnelRuntimeError::runtime_unavailable("session map is unavailable"))?
            .insert(session_id, session.clone());
        self.stats.session_opened(rule_id);
        let reader_task = self.spawn_session_reader(reader, session_id, rule_id, FrameLeg::Egress);
        self.spawn_session_writer(writer, rx, rule_id, pending_bytes);
        session.set_reader_task(reader_task);

        Ok(vec![KtpFrame {
            frame_type: FrameType::SessionAccept,
            leg: FrameLeg::Egress,
            flags: 0,
            session_id,
            payload: encode_session_accept_payload(rule_id),
        }])
    }

    async fn attach_ingress_stream(
        &self,
        session_id: u64,
        rule_id: u64,
        listen_host: String,
        listen_port: u16,
        stream: TcpStream,
        source_addr: String,
    ) -> Result<(), TunnelRuntimeError> {
        self.ensure_agent_session_capacity()?;
        let (reader, writer) = stream.into_split();
        let (tx, rx) = mpsc::channel::<Vec<u8>>(64);
        let pending_bytes = Arc::new(AtomicUsize::new(0));

        let session = AsyncTunnelSession::new(rule_id, tx, Arc::clone(&pending_bytes));
        self.sessions
            .lock()
            .map_err(|_| TunnelRuntimeError::runtime_unavailable("session map is unavailable"))?
            .insert(session_id, session.clone());
        self.stats.session_opened(rule_id);

        let payload = encode_session_open_payload(&TunnelSessionOpenPayload {
            rule_id,
            listen_host,
            listen_port,
            source_addr,
        })
        .map_err(|error| TunnelRuntimeError::runtime_unavailable(error.to_string()))?;
        self.outbound.try_push(KtpFrame {
            frame_type: FrameType::SessionOpen,
            leg: FrameLeg::Ingress,
            flags: 0,
            session_id,
            payload,
        })?;
        let reader_task = self.spawn_session_reader(reader, session_id, rule_id, FrameLeg::Ingress);
        self.spawn_session_writer(writer, rx, rule_id, pending_bytes);
        session.set_reader_task(reader_task);
        Ok(())
    }

    pub async fn handle_session_data(
        &self,
        session_id: u64,
        _leg: FrameLeg,
        payload: Vec<u8>,
    ) -> Result<(), TunnelRuntimeError> {
        let session = self
            .sessions
            .lock()
            .map_err(|_| TunnelRuntimeError::runtime_unavailable("session map is unavailable"))?
            .get(&session_id)
            .cloned()
            .ok_or_else(|| TunnelRuntimeError::runtime_unavailable("session not found"))?;

        let payload_len = payload.len();
        session.reserve_pending_bytes(payload_len, self.limits.max_session_pending_bytes)?;
        match session.to_tcp.try_send(payload) {
            Ok(()) => Ok(()),
            Err(error) => {
                session.release_pending_bytes(error.into_inner().len());
                Err(TunnelRuntimeError::backpressure_limit())
            }
        }
    }

    pub async fn next_frame(&self) -> Option<KtpFrame> {
        self.outbound.pop()
    }

    pub async fn next_frames(&self, max_frames: usize) -> Vec<KtpFrame> {
        self.outbound.drain(max_frames)
    }

    pub async fn next_frames_after_wait(
        &self,
        max_frames: usize,
        timeout: Duration,
    ) -> Vec<KtpFrame> {
        self.outbound.drain_after_wait(max_frames, timeout)
    }

    pub fn effective_outbound_batch_frames(&self, configured_batch_frames: usize) -> usize {
        let stats = self.stats_snapshot();
        self.limits
            .relay_batch_policy
            .effective_batch_frames_with_tuning(
                configured_batch_frames,
                stats.active_sessions,
                stats.recent_outbound_queue_dwell,
                self.limits.relay_batch_tuning,
            )
    }

    pub async fn close_session(
        &self,
        session_id: u64,
        _reason: &str,
    ) -> Result<(), TunnelRuntimeError> {
        let removed = self
            .sessions
            .lock()
            .map_err(|_| TunnelRuntimeError::runtime_unavailable("session map is unavailable"))?
            .remove(&session_id);
        if let Some(session) = removed {
            if let Some(reader) = session.abort_reader() {
                let _ = reader.await;
            }
            self.outbound.remove_session_frames(session_id);
            self.stats.session_closed(session.rule_id);
            Ok(())
        } else {
            Err(TunnelRuntimeError::runtime_unavailable("session not found"))
        }
    }

    pub async fn close_all_sessions(&self, _reason: &str) -> Result<(), TunnelRuntimeError> {
        let removed = self
            .sessions
            .lock()
            .map_err(|_| TunnelRuntimeError::runtime_unavailable("session map is unavailable"))?
            .drain()
            .map(|(_, session)| session)
            .collect::<Vec<_>>();
        let mut reader_tasks = Vec::new();
        for session in removed {
            if let Some(reader) = session.abort_reader() {
                reader_tasks.push(reader);
            }
            self.stats.session_closed(session.rule_id);
        }
        for reader in reader_tasks {
            let _ = reader.await;
        }
        self.outbound.clear();
        Ok(())
    }

    pub fn stats_snapshot(&self) -> TunnelRuntimeStatsSnapshot {
        self.stats.snapshot()
    }

    fn ensure_agent_session_capacity(&self) -> Result<(), TunnelRuntimeError> {
        if self.stats.snapshot().active_sessions >= self.limits.max_sessions_per_agent {
            return Err(TunnelRuntimeError::session_limit(
                "agent tunnel session limit reached",
            ));
        }
        Ok(())
    }

    fn spawn_session_reader(
        &self,
        mut reader: tokio::net::tcp::OwnedReadHalf,
        session_id: u64,
        rule_id: u64,
        leg: FrameLeg,
    ) -> tokio::task::JoinHandle<()> {
        let outbound = self.outbound.clone();
        let sessions = self.sessions.clone();
        let stats = self.stats.clone();
        let chunk_size = self.limits.tcp_read_chunk_size;
        tokio::spawn(async move {
            let mut buffer = vec![0u8; chunk_size];
            loop {
                match reader.read(&mut buffer).await {
                    Ok(0) => {
                        let _ = outbound.try_push(KtpFrame {
                            frame_type: FrameType::SessionClose,
                            leg,
                            flags: 0,
                            session_id,
                            payload: Vec::new(),
                        });
                        break;
                    }
                    Ok(read) => {
                        stats.bytes_out(rule_id, read as u64);
                        let _ = outbound.try_push(KtpFrame {
                            frame_type: FrameType::SessionData,
                            leg,
                            flags: 0,
                            session_id,
                            payload: buffer[..read].to_vec(),
                        });
                    }
                    Err(_) => break,
                }
            }
            let removed = sessions
                .lock()
                .ok()
                .and_then(|mut sessions| sessions.remove(&session_id));
            if removed.is_some() {
                stats.session_closed(rule_id);
            }
        })
    }

    fn spawn_session_writer(
        &self,
        mut writer: tokio::net::tcp::OwnedWriteHalf,
        mut rx: mpsc::Receiver<Vec<u8>>,
        rule_id: u64,
        pending_bytes: Arc<AtomicUsize>,
    ) {
        let stats = self.stats.clone();
        tokio::spawn(async move {
            while let Some(payload) = rx.recv().await {
                let payload_len = payload.len();
                stats.bytes_in(rule_id, payload.len() as u64);
                let write_result = writer.write_all(&payload).await;
                release_pending_bytes(&pending_bytes, payload_len);
                if write_result.is_err() {
                    break;
                }
            }
        });
    }
}

impl AsyncTunnelSession {
    fn new(rule_id: u64, to_tcp: mpsc::Sender<Vec<u8>>, pending_bytes: Arc<AtomicUsize>) -> Self {
        Self {
            rule_id,
            to_tcp,
            pending_bytes,
            tasks: Arc::new(Mutex::new(AsyncTunnelSessionTasks::default())),
        }
    }

    fn set_reader_task(&self, reader: tokio::task::JoinHandle<()>) {
        if let Ok(mut tasks) = self.tasks.lock() {
            tasks.reader = Some(reader);
        }
    }

    fn abort_reader(&self) -> Option<tokio::task::JoinHandle<()>> {
        if let Ok(mut tasks) = self.tasks.lock() {
            if let Some(reader) = tasks.reader.take() {
                reader.abort();
                return Some(reader);
            }
        }
        None
    }

    fn reserve_pending_bytes(&self, bytes: usize, limit: usize) -> Result<(), TunnelRuntimeError> {
        loop {
            let current = self.pending_bytes.load(Ordering::Acquire);
            let Some(next) = current.checked_add(bytes) else {
                return Err(TunnelRuntimeError::session_pending_bytes_limit(
                    "session pending byte counter overflowed",
                ));
            };
            if next > limit {
                return Err(TunnelRuntimeError::session_pending_bytes_limit(format!(
                    "session pending bytes would exceed limit {limit}"
                )));
            }
            if self
                .pending_bytes
                .compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Ok(());
            }
        }
    }

    fn release_pending_bytes(&self, bytes: usize) {
        release_pending_bytes(&self.pending_bytes, bytes);
    }
}

fn release_pending_bytes(counter: &AtomicUsize, bytes: usize) {
    let _ = counter.fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
        Some(current.saturating_sub(bytes))
    });
}

impl TunnelRuntimeError {
    pub fn target_connect_failed(message: impl Into<String>) -> Self {
        Self {
            code: "target_connect_failed",
            message: message.into(),
        }
    }

    pub fn listen_bind_failed(message: impl Into<String>) -> Self {
        Self {
            code: "listen_bind_failed",
            message: message.into(),
        }
    }

    pub fn session_limit(message: impl Into<String>) -> Self {
        Self {
            code: "session_limit",
            message: message.into(),
        }
    }

    pub fn session_pending_bytes_limit(message: impl Into<String>) -> Self {
        Self {
            code: "session_pending_bytes_limit",
            message: message.into(),
        }
    }
}
