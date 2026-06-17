use crate::ktp::{FrameLeg, FrameType, KtpFrame};
use crate::tunnel_session::encode_session_accept_payload;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
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
}

#[derive(Clone, Debug)]
pub struct AsyncTunnelFrameQueue {
    inner: Arc<Mutex<VecDeque<KtpFrame>>>,
    capacity: usize,
}

impl AsyncTunnelFrameQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::new())),
            capacity,
        }
    }

    pub fn try_push(&self, frame: KtpFrame) -> Result<(), TunnelRuntimeError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| TunnelRuntimeError::runtime_unavailable("frame queue is unavailable"))?;
        if inner.len() >= self.capacity {
            return Err(TunnelRuntimeError::backpressure_limit());
        }
        inner.push_back(frame);
        Ok(())
    }

    pub fn pop(&self) -> Option<KtpFrame> {
        self.inner
            .lock()
            .ok()
            .and_then(|mut inner| inner.pop_front())
    }

    pub fn len(&self) -> usize {
        self.inner.lock().map(|inner| inner.len()).unwrap_or(0)
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

#[derive(Clone, Debug)]
pub struct AsyncTunnelCore {
    limits: TunnelRuntimeLimits,
    outbound: AsyncTunnelFrameQueue,
    sessions: Arc<Mutex<HashMap<u64, mpsc::Sender<Vec<u8>>>>>,
    stats: TunnelRuntimeStats,
}

impl AsyncTunnelCore {
    pub fn new(limits: TunnelRuntimeLimits) -> Self {
        Self {
            outbound: AsyncTunnelFrameQueue::new(limits.max_outbound_frames),
            limits,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            stats: TunnelRuntimeStats::default(),
        }
    }

    pub async fn open_egress_session(
        &self,
        session_id: u64,
        rule_id: u64,
        target_host: &str,
        target_port: u16,
        _open_payload: Vec<u8>,
    ) -> Result<Vec<KtpFrame>, TunnelRuntimeError> {
        let target = format!("{}:{}", target_host.trim(), target_port);
        let stream =
            tokio::time::timeout(self.limits.target_dial_timeout, TcpStream::connect(&target))
                .await
                .map_err(|_| TunnelRuntimeError::target_connect_failed("target dial timed out"))?
                .map_err(|error| TunnelRuntimeError::target_connect_failed(error.to_string()))?;
        let (reader, writer) = stream.into_split();
        let (tx, rx) = mpsc::channel::<Vec<u8>>(64);

        self.sessions
            .lock()
            .map_err(|_| TunnelRuntimeError::runtime_unavailable("session map is unavailable"))?
            .insert(session_id, tx);
        self.stats.session_opened(rule_id);
        self.spawn_session_reader(reader, session_id, rule_id, FrameLeg::Egress);
        self.spawn_session_writer(writer, rx, rule_id);

        Ok(vec![KtpFrame {
            frame_type: FrameType::SessionAccept,
            leg: FrameLeg::Egress,
            flags: 0,
            session_id,
            payload: encode_session_accept_payload(rule_id),
        }])
    }

    pub async fn handle_session_data(
        &self,
        session_id: u64,
        _leg: FrameLeg,
        payload: Vec<u8>,
    ) -> Result<(), TunnelRuntimeError> {
        let sender = self
            .sessions
            .lock()
            .map_err(|_| TunnelRuntimeError::runtime_unavailable("session map is unavailable"))?
            .get(&session_id)
            .cloned()
            .ok_or_else(|| TunnelRuntimeError::runtime_unavailable("session not found"))?;

        sender
            .try_send(payload)
            .map_err(|_| TunnelRuntimeError::backpressure_limit())
    }

    pub async fn next_frame(&self) -> Option<KtpFrame> {
        self.outbound.pop()
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
            if let Ok(mut sessions) = sessions.lock() {
                sessions.remove(&session_id);
            }
            stats.session_closed(rule_id);
        });
    }

    fn spawn_session_writer(
        &self,
        mut writer: tokio::net::tcp::OwnedWriteHalf,
        mut rx: mpsc::Receiver<Vec<u8>>,
        rule_id: u64,
    ) {
        let stats = self.stats.clone();
        tokio::spawn(async move {
            while let Some(payload) = rx.recv().await {
                stats.bytes_in(rule_id, payload.len() as u64);
                if writer.write_all(&payload).await.is_err() {
                    break;
                }
            }
        });
    }
}

impl TunnelRuntimeError {
    pub fn target_connect_failed(message: impl Into<String>) -> Self {
        Self {
            code: "target_connect_failed",
            message: message.into(),
        }
    }
}
