# KTP Encrypted TCP Relay Simulation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a testable two-endpoint relay simulation for the encrypted KTP TCP carrier without replacing the production WebSocket relay path.

**Architecture:** Keep `KtpEncryptedTcpStream` as the per-connection carrier. Add a small `KtpEncryptedTcpFrameRelay` that owns two encrypted streams and can forward the next frame from left to right or right to left. This creates an integration seam for a future backend relay while keeping business routing, auth, and control-plane negotiation outside this low-level transport module.

**Tech Stack:** Rust 2021, Tokio TCP, existing KTP crypto carrier, Cargo integration tests.

---

## File Structure

- Modify `src/ktp_transport.rs`
  - Add `KtpEncryptedTcpFrameRelay` and `KtpEncryptedTcpRelayStats`.
- Modify `tests/ktp_transport.rs`
  - Add two-endpoint encrypted relay simulation test.

## Task 1: Two-Endpoint Relay Bridge

**Files:**
- Modify: `src/ktp_transport.rs`
- Modify: `tests/ktp_transport.rs`

- [ ] **Step 1: Write failing relay test**

Append to `tests/ktp_transport.rs`:

```rust
use kelicloud_agent_rs::ktp_transport::KtpEncryptedTcpFrameRelay;

#[test]
fn encrypted_tcp_frame_relay_forwards_between_two_endpoints() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .worker_threads(4)
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let key = test_crypto_key();
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let relay_key = key.clone();
        let relay_task = tokio::spawn(async move {
            let (left_stream, _) = listener.accept().await.expect("accept left");
            let (right_stream, _) = listener.accept().await.expect("accept right");
            let left = KtpEncryptedTcpStream::from_stream(
                left_stream,
                relay_key.clone(),
                KtpCryptoDirection::RelayToClient,
                KtpCryptoDirection::ClientToRelay,
                KTP_MAX_PAYLOAD_LEN,
                1024 * 1024,
            );
            let right = KtpEncryptedTcpStream::from_stream(
                right_stream,
                relay_key,
                KtpCryptoDirection::RelayToClient,
                KtpCryptoDirection::ClientToRelay,
                KTP_MAX_PAYLOAD_LEN,
                1024 * 1024,
            );
            let mut relay = KtpEncryptedTcpFrameRelay::new(left, right);
            let forwarded = relay
                .relay_next_left_to_right()
                .await
                .expect("relay request");
            assert_eq!(forwarded.session_id, 1201);
            let forwarded = relay
                .relay_next_right_to_left()
                .await
                .expect("relay response");
            assert_eq!(forwarded.session_id, 1202);
            assert_eq!(relay.stats().frames_left_to_right, 1);
            assert_eq!(relay.stats().frames_right_to_left, 1);
        });

        let mut left_client = connect_encrypted_client(addr, key.clone()).await;
        let mut right_client = connect_encrypted_client(addr, key).await;
        left_client
            .send_frame(&session_data(1201, b"from left"))
            .await
            .expect("send left");
        assert_eq!(
            right_client.next_frame().await.expect("right receives"),
            session_data(1201, b"from left")
        );
        right_client
            .send_frame(&session_data(1202, b"from right"))
            .await
            .expect("send right");
        assert_eq!(
            left_client.next_frame().await.expect("left receives"),
            session_data(1202, b"from right")
        );
        relay_task.await.expect("relay task");
    });
}

async fn connect_encrypted_client(
    addr: std::net::SocketAddr,
    key: KtpCryptoKey,
) -> KtpEncryptedTcpStream {
    let stream = TcpStream::connect(addr).await.expect("connect client");
    KtpEncryptedTcpStream::from_stream(
        stream,
        key,
        KtpCryptoDirection::ClientToRelay,
        KtpCryptoDirection::RelayToClient,
        KTP_MAX_PAYLOAD_LEN,
        1024 * 1024,
    )
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test --test ktp_transport encrypted_tcp_frame_relay_forwards_between_two_endpoints -- --exact --nocapture
```

Expected: FAIL because `KtpEncryptedTcpFrameRelay` does not exist.

- [ ] **Step 3: Implement minimal relay bridge**

Modify `src/ktp_transport.rs`:

```rust
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KtpEncryptedTcpRelayStats {
    pub frames_left_to_right: u64,
    pub frames_right_to_left: u64,
}

pub struct KtpEncryptedTcpFrameRelay {
    left: KtpEncryptedTcpStream,
    right: KtpEncryptedTcpStream,
    stats: KtpEncryptedTcpRelayStats,
}

impl KtpEncryptedTcpFrameRelay {
    pub fn new(left: KtpEncryptedTcpStream, right: KtpEncryptedTcpStream) -> Self {
        Self {
            left,
            right,
            stats: KtpEncryptedTcpRelayStats::default(),
        }
    }

    pub fn stats(&self) -> KtpEncryptedTcpRelayStats {
        self.stats
    }

    pub async fn relay_next_left_to_right(&mut self) -> Result<KtpFrame, KtpTcpTransportError> {
        let frame = self.left.next_frame().await?;
        self.right.send_frame(&frame).await?;
        self.stats.frames_left_to_right += 1;
        Ok(frame)
    }

    pub async fn relay_next_right_to_left(&mut self) -> Result<KtpFrame, KtpTcpTransportError> {
        let frame = self.right.next_frame().await?;
        self.left.send_frame(&frame).await?;
        self.stats.frames_right_to_left += 1;
        Ok(frame)
    }
}
```

- [ ] **Step 4: Run relay test**

Run:

```powershell
cargo test --test ktp_transport encrypted_tcp_frame_relay_forwards_between_two_endpoints -- --exact --nocapture
```

Expected: PASS.

- [ ] **Step 5: Run regression tests**

Run:

```powershell
cargo test --test ktp_transport --test tunnel_async_runtime --test tunnel_runtime -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```powershell
git add src/ktp_transport.rs tests/ktp_transport.rs docs/superpowers/plans/2026-06-17-ktp-encrypted-tcp-relay-simulation.md
git commit -m "feat: add encrypted ktp tcp relay simulation"
```
