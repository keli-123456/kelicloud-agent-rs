# KTP Encrypted TCP Carrier Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a local, testable encrypted TCP carrier for KTP records without replacing the current production WebSocket relay path.

**Architecture:** Keep the existing `KtpCryptoSeal` and `KtpCryptoRecordCodec` as the record layer. Add a small Tokio TCP wrapper that writes sealed records with `write_all`, reads arbitrary TCP chunks into the crypto record codec, and returns decrypted `KtpFrame` values. The wrapper is reusable by future relay/client integration but remains isolated from current agent runtime wiring in this phase.

**Tech Stack:** Rust 2021, Tokio TCP and async I/O, existing KTP crypto record layer, Cargo integration tests.

---

## File Structure

- Modify `src/ktp_transport.rs`
  - Add `KtpEncryptedTcpStream` and `KtpTcpTransportError`.
- Modify `tests/ktp_transport.rs`
  - Add loopback test using real Tokio TCP listener and stream.

## Task 1: Encrypted TCP Stream Round Trip

**Files:**
- Modify: `src/ktp_transport.rs`
- Modify: `tests/ktp_transport.rs`

- [ ] **Step 1: Write failing loopback test**

Append to `tests/ktp_transport.rs`:

```rust
use kelicloud_agent_rs::ktp_transport::KtpEncryptedTcpStream;
use tokio::net::{TcpListener, TcpStream};

#[test]
fn encrypted_tcp_stream_round_trips_frame_over_loopback() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .worker_threads(2)
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let key = test_crypto_key();
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let server_key = key.clone();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept encrypted tcp");
            let mut server = KtpEncryptedTcpStream::from_stream(
                stream,
                server_key,
                KtpCryptoDirection::RelayToClient,
                KtpCryptoDirection::ClientToRelay,
                KTP_MAX_PAYLOAD_LEN,
                1024 * 1024,
            );
            let request = server.next_frame().await.expect("read request");
            assert_eq!(request, session_data(801, b"hello relay"));
            server
                .send_frame(&session_data(802, b"hello client"))
                .await
                .expect("send response");
        });

        let stream = TcpStream::connect(addr).await.expect("connect client");
        let mut client = KtpEncryptedTcpStream::from_stream(
            stream,
            key,
            KtpCryptoDirection::ClientToRelay,
            KtpCryptoDirection::RelayToClient,
            KTP_MAX_PAYLOAD_LEN,
            1024 * 1024,
        );
        client
            .send_frame(&session_data(801, b"hello relay"))
            .await
            .expect("send request");
        assert_eq!(
            client.next_frame().await.expect("read response"),
            session_data(802, b"hello client")
        );
        server.await.expect("server task");
    });
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test --test ktp_transport encrypted_tcp_stream_round_trips_frame_over_loopback -- --exact --nocapture
```

Expected: FAIL because `KtpEncryptedTcpStream` does not exist.

- [ ] **Step 3: Implement minimal encrypted TCP stream wrapper**

Modify `src/ktp_transport.rs`:

```rust
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug)]
pub enum KtpTcpTransportError {
    Io(std::io::Error),
    Crypto(KtpCryptoError),
    Closed,
}

impl fmt::Display for KtpTcpTransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Crypto(error) => write!(f, "{error}"),
            Self::Closed => write!(f, "KTP encrypted TCP stream closed"),
        }
    }
}

impl Error for KtpTcpTransportError {}

pub struct KtpEncryptedTcpStream {
    stream: TcpStream,
    seal: KtpCryptoSeal,
    codec: KtpCryptoRecordCodec,
    read_buffer: Vec<u8>,
}

impl KtpEncryptedTcpStream {
    pub fn from_stream(
        stream: TcpStream,
        key: KtpCryptoKey,
        seal_direction: KtpCryptoDirection,
        open_direction: KtpCryptoDirection,
        max_payload_len: usize,
        max_buffer_len: usize,
    ) -> Self {
        Self {
            stream,
            seal: KtpCryptoSeal::new(key.clone(), seal_direction),
            codec: KtpCryptoRecordCodec::new(key, open_direction, max_payload_len, max_buffer_len),
            read_buffer: vec![0u8; 16 * 1024],
        }
    }

    pub async fn send_frame(&mut self, frame: &KtpFrame) -> Result<(), KtpTcpTransportError> {
        let record = self
            .seal
            .seal_frame(frame)
            .map_err(KtpTcpTransportError::Crypto)?;
        self.stream
            .write_all(&record)
            .await
            .map_err(KtpTcpTransportError::Io)
    }

    pub async fn next_frame(&mut self) -> Result<KtpFrame, KtpTcpTransportError> {
        loop {
            if let Some(frame) = self.codec.next_frame().map_err(KtpTcpTransportError::Crypto)? {
                return Ok(frame);
            }
            let read = self
                .stream
                .read(&mut self.read_buffer)
                .await
                .map_err(KtpTcpTransportError::Io)?;
            if read == 0 {
                return Err(KtpTcpTransportError::Closed);
            }
            self.codec
                .push(&self.read_buffer[..read])
                .map_err(KtpTcpTransportError::Crypto)?;
        }
    }
}
```

- [ ] **Step 4: Run loopback test**

Run:

```powershell
cargo test --test ktp_transport encrypted_tcp_stream_round_trips_frame_over_loopback -- --exact --nocapture
```

Expected: PASS.

- [ ] **Step 5: Run regression tests**

Run:

```powershell
cargo test --test ktp_transport --test tunnel_async_runtime -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```powershell
git add src/ktp_transport.rs tests/ktp_transport.rs docs/superpowers/plans/2026-06-17-ktp-encrypted-tcp-carrier.md
git commit -m "feat: add ktp encrypted tcp carrier"
```
