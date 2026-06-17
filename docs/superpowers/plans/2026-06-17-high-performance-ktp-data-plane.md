# High Performance KTP Data Plane Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first Linux-only high-performance KTP data-plane foundation by moving the Rust agent tunnel TCP runtime toward Tokio async tasks, bounded queues, deterministic cleanup, and measurable limits while keeping the current backend relay and WebSocket carrier compatible.

**Architecture:** Keep KTP and backend relay protocol stable. Introduce an async tunnel runtime core behind the existing `TunnelSessionRuntime` boundary, then migrate egress sessions, ingress listeners, lifecycle cleanup, and stress evidence in separate test-first steps.

**Tech Stack:** Rust 2021, `tokio` with `rt-multi-thread`, `net`, `sync`, `io-util`, and `time`; existing `tungstenite` WebSocket carrier; existing KTP codec and tunnel tests.

---

## File Structure

- Modify `Cargo.toml`
  - Add Tokio dependency for the internal tunnel runtime.
- Create `src/tunnel_async_runtime.rs`
  - Own async limits, stats, bounded frame queue, async listener/session core, and compatibility handle.
- Modify `src/lib.rs`
  - Export `tunnel_async_runtime`.
- Modify `src/tunnel_runtime.rs`
  - Keep public structs and `TunnelSessionRuntime` trait stable.
  - Replace blocking session internals with an async core adapter once the core is tested.
- Modify `tests/tunnel_runtime.rs`
  - Keep existing behavior tests and add high-concurrency/limit checks.
- Create `tests/tunnel_async_runtime.rs`
  - Test async queue, limits, egress, ingress, and cleanup at the core boundary.
- Modify `scripts/tunnel-relay-local-smoke.sh`
  - Add async runtime and concurrency checks.
- Modify `tests/tunnel_relay_smoke_script.rs`
  - Ensure smoke script runs the new async runtime tests.

## Task 1: Async Runtime Configuration And Queue Foundation

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/lib.rs`
- Create: `src/tunnel_async_runtime.rs`
- Test: `tests/tunnel_async_runtime.rs`

- [ ] **Step 1: Write failing async runtime foundation tests**

Create `tests/tunnel_async_runtime.rs`:

```rust
use kelicloud_agent_rs::ktp::{FrameLeg, FrameType, KtpFrame};
use kelicloud_agent_rs::tunnel_async_runtime::{
    AsyncTunnelFrameQueue, TunnelRuntimeLimits, TunnelRuntimeStats,
};

#[test]
fn async_runtime_limits_have_bounded_defaults() {
    let limits = TunnelRuntimeLimits::default();

    assert_eq!(limits.max_sessions_per_agent, 1024);
    assert_eq!(limits.max_outbound_frames, 4096);
    assert_eq!(limits.max_session_pending_bytes, 4 * 1024 * 1024);
    assert_eq!(limits.tcp_read_chunk_size, 16 * 1024);
    assert!(limits.target_dial_timeout.as_secs() <= 5);
    assert!(limits.idle_timeout.as_secs() >= 600);
}

#[test]
fn async_frame_queue_enforces_frame_capacity() {
    let queue = AsyncTunnelFrameQueue::new(2);
    queue
        .try_push(frame(1, b"a"))
        .expect("first frame should fit");
    queue
        .try_push(frame(2, b"b"))
        .expect("second frame should fit");

    let err = queue
        .try_push(frame(3, b"c"))
        .expect_err("third frame should exceed capacity");

    assert_eq!(err.code(), "backpressure_limit");
    assert_eq!(queue.len(), 2);
}

#[test]
fn runtime_stats_snapshot_tracks_session_and_byte_counters() {
    let stats = TunnelRuntimeStats::default();
    stats.session_opened(7);
    stats.bytes_in(7, 12);
    stats.bytes_out(7, 34);
    stats.session_closed(7);

    let snapshot = stats.snapshot();

    assert_eq!(snapshot.active_sessions, 0);
    assert_eq!(snapshot.total_sessions, 1);
    assert_eq!(snapshot.bytes_in, 12);
    assert_eq!(snapshot.bytes_out, 34);
    assert_eq!(snapshot.rule_session_counts.get(&7).copied(), Some(0));
}

fn frame(session_id: u64, payload: &[u8]) -> KtpFrame {
    KtpFrame {
        frame_type: FrameType::SessionData,
        leg: FrameLeg::Ingress,
        flags: 0,
        session_id,
        payload: payload.to_vec(),
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test --test tunnel_async_runtime -- --nocapture
```

Expected: FAIL because `tunnel_async_runtime` does not exist.

- [ ] **Step 3: Add Tokio dependency and async runtime module skeleton**

Add to `Cargo.toml`:

```toml
tokio = { version = "1", features = ["rt-multi-thread", "net", "sync", "io-util", "time"] }
```

Add to `src/lib.rs`:

```rust
pub mod tunnel_async_runtime;
```

Create `src/tunnel_async_runtime.rs`:

```rust
use crate::ktp::KtpFrame;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
        let Ok(mut inner) = self.inner.lock() else {
            return Err(TunnelRuntimeError {
                code: "runtime_unavailable",
                message: "tunnel frame queue is unavailable".to_string(),
            });
        };
        if inner.len() >= self.capacity {
            return Err(TunnelRuntimeError::backpressure_limit());
        }
        inner.push_back(frame);
        Ok(())
    }

    pub fn pop(&self) -> Option<KtpFrame> {
        self.inner.lock().ok().and_then(|mut inner| inner.pop_front())
    }

    pub fn len(&self) -> usize {
        self.inner.lock().map(|inner| inner.len()).unwrap_or(0)
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
        self.active_sessions.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            Some(value.saturating_sub(1))
        }).ok();
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
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```powershell
cargo test --test tunnel_async_runtime -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add Cargo.toml Cargo.lock src/lib.rs src/tunnel_async_runtime.rs tests/tunnel_async_runtime.rs
git commit -m "feat: add async tunnel runtime foundation"
```

## Task 2: Async Egress Session Core

**Files:**
- Modify: `src/tunnel_async_runtime.rs`
- Test: `tests/tunnel_async_runtime.rs`

- [ ] **Step 1: Write failing async egress echo test**

Append to `tests/tunnel_async_runtime.rs`:

```rust
use kelicloud_agent_rs::tunnel_async_runtime::AsyncTunnelCore;
use kelicloud_agent_rs::tunnel_session::{
    encode_session_open_payload, TunnelSessionOpenPayload,
};
use std::net::TcpListener;
use std::thread;
use std::io::{Read, Write};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn async_egress_session_connects_target_and_queues_response() {
    let target = TcpListener::bind("127.0.0.1:0").expect("bind target");
    let target_port = target.local_addr().expect("target addr").port();
    let echo = thread::spawn(move || {
        let (mut stream, _) = target.accept().expect("accept target");
        let mut buffer = [0u8; 16];
        let read = stream.read(&mut buffer).expect("read target");
        assert_eq!(&buffer[..read], b"ping");
        stream.write_all(b"pong").expect("write target");
    });

    let core = AsyncTunnelCore::new(TunnelRuntimeLimits::default());
    let open_payload = encode_session_open_payload(&TunnelSessionOpenPayload {
        rule_id: 7,
        listen_host: "127.0.0.1".to_string(),
        listen_port: 10088,
        source_addr: "127.0.0.1:50123".to_string(),
    }).expect("encode open");

    let responses = core
        .open_egress_session(77, 7, "127.0.0.1", target_port, open_payload)
        .await
        .expect("open egress");
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0].frame_type, FrameType::SessionAccept);

    core.handle_session_data(77, FrameLeg::Egress, b"ping".to_vec())
        .await
        .expect("write target data");

    let frame = core.next_frame().await.expect("target response frame");
    assert_eq!(frame.frame_type, FrameType::SessionData);
    assert_eq!(frame.leg, FrameLeg::Egress);
    assert_eq!(frame.session_id, 77);
    assert_eq!(frame.payload, b"pong");
    echo.join().expect("echo thread");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test --test tunnel_async_runtime async_egress_session_connects_target_and_queues_response -- --nocapture
```

Expected: FAIL because `AsyncTunnelCore` does not exist.

- [ ] **Step 3: Implement minimal async egress core**

Add to `src/tunnel_async_runtime.rs`:

```rust
use crate::ktp::{FrameLeg, FrameType};
use crate::tunnel_session::encode_session_accept_payload;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

#[derive(Clone)]
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
        let stream = tokio::time::timeout(
            self.limits.target_dial_timeout,
            TcpStream::connect(format!("{target_host}:{target_port}")),
        )
        .await
        .map_err(|_| TunnelRuntimeError {
            code: "target_connect_failed",
            message: "target dial timed out".to_string(),
        })?
        .map_err(|error| TunnelRuntimeError {
            code: "target_connect_failed",
            message: error.to_string(),
        })?;

        let (reader, writer) = stream.into_split();
        let (tx, rx) = mpsc::channel::<Vec<u8>>(64);
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.insert(session_id, tx);
        }
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
            .ok()
            .and_then(|sessions| sessions.get(&session_id).cloned())
            .ok_or_else(|| TunnelRuntimeError {
                code: "runtime_unavailable",
                message: "session not found".to_string(),
            })?;
        sender.try_send(payload).map_err(|_| TunnelRuntimeError::backpressure_limit())
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
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```powershell
cargo test --test tunnel_async_runtime async_egress_session_connects_target_and_queues_response -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src/tunnel_async_runtime.rs tests/tunnel_async_runtime.rs
git commit -m "feat: add async tunnel egress session core"
```

## Task 3: Async Ingress Listener Core

**Files:**
- Modify: `src/tunnel_async_runtime.rs`
- Test: `tests/tunnel_async_runtime.rs`

- [ ] **Step 1: Write failing async ingress listener test**

Append to `tests/tunnel_async_runtime.rs`:

```rust
use kelicloud_agent_rs::tunnel_async_runtime::TunnelIngressListenerSpec;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream as TokioTcpStream;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn async_ingress_listener_queues_open_data_and_writes_response() {
    let listen_port = free_tcp_port();
    let core = AsyncTunnelCore::new(TunnelRuntimeLimits::default());
    core.start_ingress_listener(TunnelIngressListenerSpec {
        rule_id: 17,
        listen_address: "127.0.0.1".to_string(),
        listen_port,
        source_allowlist: "127.0.0.0/8".to_string(),
    })
    .await
    .expect("start listener");

    let mut client = connect_tokio_with_retry("127.0.0.1", listen_port).await;
    client.write_all(b"hello").await.expect("write client data");

    let open = wait_for_core_frame(&core).await.expect("open frame");
    assert_eq!(open.frame_type, FrameType::SessionOpen);
    assert_eq!(open.leg, FrameLeg::Ingress);
    assert_ne!(open.session_id, 0);

    let data = wait_for_core_frame(&core).await.expect("data frame");
    assert_eq!(data.frame_type, FrameType::SessionData);
    assert_eq!(data.session_id, open.session_id);
    assert_eq!(data.payload, b"hello");

    core.handle_session_data(open.session_id, FrameLeg::Ingress, b"world".to_vec())
        .await
        .expect("write response");

    let mut buffer = [0u8; 16];
    let read = client.read(&mut buffer).await.expect("read response");
    assert_eq!(&buffer[..read], b"world");
}

async fn connect_tokio_with_retry(host: &str, port: u16) -> TokioTcpStream {
    let addr = format!("{host}:{port}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        match TokioTcpStream::connect(&addr).await {
            Ok(stream) => return stream,
            Err(error) if std::time::Instant::now() < deadline => {
                let _ = error;
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            Err(error) => panic!("connect {addr}: {error}"),
        }
    }
}

async fn wait_for_core_frame(core: &AsyncTunnelCore) -> Option<KtpFrame> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        if let Some(frame) = core.next_frame().await {
            return Some(frame);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test --test tunnel_async_runtime async_ingress_listener_queues_open_data_and_writes_response -- --nocapture
```

Expected: FAIL because `start_ingress_listener` is not implemented.

- [ ] **Step 3: Implement async listener handle**

Add this public listener spec to `src/tunnel_async_runtime.rs`:

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TunnelIngressListenerSpec {
    pub rule_id: u64,
    pub listen_address: String,
    pub listen_port: u16,
    pub source_allowlist: String,
}
```

Extend `AsyncTunnelCore` with an async listener map:

```rust
listeners: Arc<Mutex<HashMap<u64, tokio::task::JoinHandle<()>>>>,
next_session_id: Arc<AtomicU64>,
```

Implement:

```rust
pub async fn start_ingress_listener(
    &self,
    spec: TunnelIngressListenerSpec,
) -> Result<(), TunnelRuntimeError> {
    let listener = tokio::net::TcpListener::bind(format!(
        "{}:{}",
        spec.listen_address.trim(),
        spec.listen_port
    ))
    .await
    .map_err(|error| TunnelRuntimeError {
        code: "listen_bind_failed",
        message: error.to_string(),
    })?;

    let core = self.clone();
    let rule_id = spec.rule_id;
    let handle = tokio::spawn(async move {
        loop {
            let Ok((stream, peer)) = listener.accept().await else {
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            };
            if !crate::tunnel_runtime::source_addr_allowed(
                &peer.to_string(),
                &spec.source_allowlist,
            ) {
                continue;
            }
            let session_id = core.next_session_id.fetch_add(1, Ordering::Relaxed);
            let _ = core.attach_ingress_stream(session_id, rule_id, stream, peer.to_string()).await;
        }
    });

    if let Ok(mut listeners) = self.listeners.lock() {
        if let Some(previous) = listeners.insert(rule_id, handle) {
            previous.abort();
        }
    }
    Ok(())
}
```

`attach_ingress_stream` should split the stream, insert a bounded writer channel
into the session map, push `SESSION_OPEN`, then reuse the same reader/writer
task helpers created in Task 2.

- [ ] **Step 4: Run ingress test**

Run:

```powershell
cargo test --test tunnel_async_runtime async_ingress_listener_queues_open_data_and_writes_response -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src/tunnel_async_runtime.rs tests/tunnel_async_runtime.rs
git commit -m "feat: add async tunnel ingress listener core"
```

## Task 4: Compatibility Adapter For Existing TunnelSessionRuntime

**Files:**
- Modify: `src/tunnel_runtime.rs`
- Modify: `tests/tunnel_runtime.rs`

- [ ] **Step 1: Run current compatibility test before the adapter change**

Run the existing relay simulation test as a baseline:

```powershell
cargo test --test tunnel_runtime tcp_runtime_two_agent_relay_simulation_forwards_echo -- --nocapture
```

Expected: PASS on the current blocking runtime. After the adapter change, this
same test must still pass without changing its assertions.

- [ ] **Step 2: Migrate `TunnelTcpRuntime` internals**

Keep the public type name and methods. Internally store:

```rust
runtime: tokio::runtime::Runtime,
core: AsyncTunnelCore,
```

Use `runtime.block_on(...)` in `refresh_listeners`, `handle_server_frame`, and
`next_client_frame` to bridge the existing synchronous WebSocket carrier with
the async session core.

- [ ] **Step 3: Run existing runtime tests**

Run:

```powershell
cargo test --test tunnel_runtime -- --nocapture
```

Expected: PASS.

- [ ] **Step 4: Commit**

```powershell
git add src/tunnel_runtime.rs tests/tunnel_runtime.rs
git commit -m "refactor: back tunnel tcp runtime with async core"
```

## Task 5: Limits, Cleanup, And 100-Session Gate

**Files:**
- Modify: `src/tunnel_async_runtime.rs`
- Modify: `tests/tunnel_async_runtime.rs`
- Modify: `tests/tunnel_runtime.rs`

- [ ] **Step 1: Write failing limit and cleanup tests**

Append these test names to `tests/tunnel_async_runtime.rs` with concrete
assertions:

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn async_runtime_rejects_session_when_agent_limit_is_reached() {
    let mut limits = TunnelRuntimeLimits::default();
    limits.max_sessions_per_agent = 1;
    let core = AsyncTunnelCore::new(limits);
    let first = loopback_egress_session(&core, 1, 7).await;
    assert!(first.is_ok());

    let err = loopback_egress_session(&core, 2, 7)
        .await
        .expect_err("second session should exceed agent limit");
    assert_eq!(err.code(), "session_limit");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn async_runtime_cleans_session_after_local_close() {
    let core = AsyncTunnelCore::new(TunnelRuntimeLimits::default());
    loopback_egress_session(&core, 10, 7).await.expect("open session");
    assert_eq!(core.stats_snapshot().active_sessions, 1);
    core.close_session(10, "test_close").await.expect("close session");
    assert_eq!(core.stats_snapshot().active_sessions, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn async_runtime_handles_100_concurrent_loopback_sessions() {
    let core = AsyncTunnelCore::new(TunnelRuntimeLimits::default());
    let mut handles = Vec::new();
    for id in 1..=100u64 {
        let cloned = core.clone();
        handles.push(tokio::spawn(async move {
            loopback_egress_session(&cloned, id, 7).await.expect("open loopback");
            cloned.handle_session_data(id, FrameLeg::Egress, b"ping".to_vec()).await.expect("send ping");
        }));
    }
    for handle in handles {
        handle.await.expect("session task");
    }
    assert!(core.stats_snapshot().total_sessions >= 100);
}
```

The helper `loopback_egress_session` should bind a local echo listener, call
`open_egress_session`, and return the result so each test exercises real TCP.

- [ ] **Step 2: Run tests to verify they fail**

Run:

```powershell
cargo test --test tunnel_async_runtime -- --nocapture
```

Expected: FAIL on missing limit/cleanup behavior.

- [ ] **Step 3: Implement limits and cleanup**

Add session registry metadata, active counts, `close_session`, and
`close_all_sessions`. Ensure every reader/writer task removes state once.

- [ ] **Step 4: Run async runtime and existing tunnel tests**

Run:

```powershell
cargo test --test tunnel_async_runtime --test tunnel_runtime -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src/tunnel_async_runtime.rs tests/tunnel_async_runtime.rs tests/tunnel_runtime.rs
git commit -m "feat: enforce async tunnel runtime limits"
```

## Task 6: Smoke Script And Backend Compatibility Verification

**Files:**
- Modify: `scripts/tunnel-relay-local-smoke.sh`
- Modify: `tests/tunnel_relay_smoke_script.rs`

- [ ] **Step 1: Write failing smoke script assertion**

Update `tests/tunnel_relay_smoke_script.rs` to require:

```rust
assert!(script.contains("cargo test --test tunnel_async_runtime"));
assert!(script.contains("async_runtime_handles_100_concurrent_loopback_sessions"));
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test --test tunnel_relay_smoke_script -- --nocapture
```

Expected: FAIL until the script references the async runtime checks.

- [ ] **Step 3: Update smoke script**

Add:

```bash
cargo test --test tunnel_async_runtime async_runtime_handles_100_concurrent_loopback_sessions -- --nocapture
cargo test --test tunnel_runtime tcp_runtime_two_agent_relay_simulation_forwards_echo -- --nocapture
```

- [ ] **Step 4: Run full verification**

Run:

```powershell
cargo test --test tunnel_async_runtime --test tunnel_runtime --test tunnel_data --test tunnel_relay_smoke_script -- --nocapture
```

If local Go exists, also run from `C:\Users\Administrator\Documents\tanzhen\kelicloud`:

```powershell
go test ./api/client -run "TestTunnelRelay|TestTunnelDataRelaySocketEncodesKTPFrames|TestHandleTunnelDataFrame|TestTunnelSession.*Payload" -count=1
```

Expected: Rust tests PASS. Go compatibility tests PASS when Go is available.

- [ ] **Step 5: Commit**

```powershell
git add scripts/tunnel-relay-local-smoke.sh tests/tunnel_relay_smoke_script.rs
git commit -m "test: add async tunnel runtime smoke checks"
```

## Completion Checklist

- [ ] `cargo test --test tunnel_async_runtime -- --nocapture`
- [ ] `cargo test --test tunnel_runtime -- --nocapture`
- [ ] `cargo test --test tunnel_data -- --nocapture`
- [ ] `cargo test --test tunnel_relay_smoke_script -- --nocapture`
- [ ] Existing backend KTP relay tests pass or are explicitly blocked by missing local Go.
- [ ] No KTP frame format change.
- [ ] No backend schema migration.
- [ ] No change to report WebSocket, task execution, ping, terminal, or auto-discovery behavior.
- [ ] At least one commit per task.
