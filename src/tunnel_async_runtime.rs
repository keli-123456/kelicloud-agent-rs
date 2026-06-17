use crate::ktp::{FrameLeg, FrameType, KtpFrame};
use crate::tunnel_session::{
    encode_session_accept_payload, encode_session_open_payload, TunnelSessionOpenPayload,
};
use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
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
        }
    }
}

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
    frames: Mutex<VecDeque<KtpFrame>>,
    ready: Condvar,
    shared_ready: Option<Arc<TunnelFrameReadyNotifier>>,
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
        Self::new_internal(capacity, None)
    }

    pub fn new_with_notifier(capacity: usize, notifier: Arc<TunnelFrameReadyNotifier>) -> Self {
        Self::new_internal(capacity, Some(notifier))
    }

    fn new_internal(capacity: usize, shared_ready: Option<Arc<TunnelFrameReadyNotifier>>) -> Self {
        Self {
            inner: Arc::new(AsyncTunnelFrameQueueInner {
                frames: Mutex::new(VecDeque::new()),
                ready: Condvar::new(),
                shared_ready,
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
            inner.push_back(frame);
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
    }

    pub fn drain(&self, max_frames: usize) -> Vec<KtpFrame> {
        if max_frames == 0 {
            return Vec::new();
        }
        let Ok(mut inner) = self.inner.frames.lock() else {
            return Vec::new();
        };
        let count = max_frames.min(inner.len());
        inner.drain(..count).collect()
    }

    pub fn drain_after_wait(&self, max_frames: usize, timeout: Duration) -> Vec<KtpFrame> {
        if max_frames == 0 {
            return Vec::new();
        }
        let Ok(mut inner) = self.inner.frames.lock() else {
            return Vec::new();
        };
        if inner.is_empty() && !timeout.is_zero() {
            let Ok((waited_inner, _)) = self.inner.ready.wait_timeout(inner, timeout) else {
                return Vec::new();
            };
            inner = waited_inner;
        }
        let count = max_frames.min(inner.len());
        inner.drain(..count).collect()
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
}

#[derive(Clone, Debug, Default)]
pub struct TunnelRuntimeStats {
    active_sessions: Arc<AtomicUsize>,
    total_sessions: Arc<AtomicU64>,
    bytes_in: Arc<AtomicU64>,
    bytes_out: Arc<AtomicU64>,
    rule_session_counts: Arc<Mutex<HashMap<u64, usize>>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TunnelRuntimeStatsSnapshot {
    pub active_sessions: usize,
    pub total_sessions: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub rule_session_counts: HashMap<u64, usize>,
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

    pub fn snapshot(&self) -> TunnelRuntimeStatsSnapshot {
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
        }
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
        Self {
            outbound: match frame_ready_notifier {
                Some(notifier) => {
                    AsyncTunnelFrameQueue::new_with_notifier(limits.max_outbound_frames, notifier)
                }
                None => AsyncTunnelFrameQueue::new(limits.max_outbound_frames),
            },
            limits,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            listeners: Arc::new(Mutex::new(HashMap::new())),
            next_session_id: Arc::new(AtomicU64::new(1)),
            stats: TunnelRuntimeStats::default(),
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

        self.sessions
            .lock()
            .map_err(|_| TunnelRuntimeError::runtime_unavailable("session map is unavailable"))?
            .insert(
                session_id,
                AsyncTunnelSession {
                    rule_id,
                    to_tcp: tx,
                    pending_bytes: Arc::clone(&pending_bytes),
                },
            );
        self.stats.session_opened(rule_id);
        self.spawn_session_reader(reader, session_id, rule_id, FrameLeg::Egress);
        self.spawn_session_writer(writer, rx, rule_id, pending_bytes);

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

        self.sessions
            .lock()
            .map_err(|_| TunnelRuntimeError::runtime_unavailable("session map is unavailable"))?
            .insert(
                session_id,
                AsyncTunnelSession {
                    rule_id,
                    to_tcp: tx,
                    pending_bytes: Arc::clone(&pending_bytes),
                },
            );
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
        self.spawn_session_reader(reader, session_id, rule_id, FrameLeg::Ingress);
        self.spawn_session_writer(writer, rx, rule_id, pending_bytes);
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
            self.stats.session_closed(session.rule_id);
            Ok(())
        } else {
            Err(TunnelRuntimeError::runtime_unavailable("session not found"))
        }
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
    ) {
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
        });
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
