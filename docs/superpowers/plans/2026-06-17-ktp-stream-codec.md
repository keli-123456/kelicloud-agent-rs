# KTP Stream Codec Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a standalone KTP stream codec that can frame existing KTP messages over raw TCP or TLS carriers without changing the current WebSocket relay path.

**Architecture:** Reuse the existing `ktp` frame format and add a small streaming decoder on top of it. The codec buffers partial TCP chunks, returns whole `KtpFrame` values, rejects oversized payloads from the header before buffering the body, and leaves existing runtime/data paths untouched until a later integration phase.

**Tech Stack:** Rust 2021, existing `ktp::{encode_frame, decode_frame}`, standard library buffers, existing Cargo test workflow.

---

## File Structure

- Create `src/ktp_transport.rs`
  - Own `KtpStreamCodec`, `KtpStreamCodecError`, bounded push, and `next_frame`.
- Modify `src/lib.rs`
  - Export `pub mod ktp_transport;`.
- Create `tests/ktp_transport.rs`
  - Verify partial chunks, multiple frames in one chunk, and early oversized payload rejection.

## Task 1: Streaming Decode Boundary

**Files:**
- Create: `tests/ktp_transport.rs`
- Create: `src/ktp_transport.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write failing stream codec tests**

Create `tests/ktp_transport.rs`:

```rust
use kelicloud_agent_rs::ktp::{
    encode_frame, FrameLeg, FrameType, KtpError, KtpFrame, KTP_HEADER_LEN, KTP_MAX_PAYLOAD_LEN,
    KTP_VERSION,
};
use kelicloud_agent_rs::ktp_transport::{KtpStreamCodec, KtpStreamCodecError};

#[test]
fn stream_codec_decodes_frame_split_across_tcp_chunks() {
    let frame = session_data(42, b"hello");
    let bytes = encode_frame(&frame).expect("encode frame");
    let mut codec = KtpStreamCodec::new(KTP_MAX_PAYLOAD_LEN, 1024 * 1024);

    codec.push(&bytes[..7]).expect("push first chunk");
    assert_eq!(codec.next_frame().expect("decode first chunk"), None);

    codec.push(&bytes[7..]).expect("push second chunk");
    assert_eq!(codec.next_frame().expect("decode frame"), Some(frame));
    assert_eq!(codec.next_frame().expect("decode empty"), None);
}

#[test]
fn stream_codec_decodes_multiple_frames_from_one_chunk() {
    let first = session_data(7, b"one");
    let second = session_data(8, b"two");
    let mut bytes = encode_frame(&first).expect("encode first");
    bytes.extend_from_slice(&encode_frame(&second).expect("encode second"));
    let mut codec = KtpStreamCodec::new(KTP_MAX_PAYLOAD_LEN, 1024 * 1024);

    codec.push(&bytes).expect("push combined chunk");

    assert_eq!(codec.next_frame().expect("decode first"), Some(first));
    assert_eq!(codec.next_frame().expect("decode second"), Some(second));
    assert_eq!(codec.next_frame().expect("decode empty"), None);
}

#[test]
fn stream_codec_rejects_oversized_payload_from_header_before_body_arrives() {
    let mut header = Vec::new();
    header.extend_from_slice(b"KTP1");
    header.push(KTP_VERSION);
    header.push(FrameType::SessionData as u8);
    header.push(FrameLeg::Ingress as u8);
    header.push(0);
    header.extend_from_slice(&9u64.to_be_bytes());
    header.extend_from_slice(&11u32.to_be_bytes());
    header.extend_from_slice(&0u32.to_be_bytes());
    assert_eq!(header.len(), KTP_HEADER_LEN);
    let mut codec = KtpStreamCodec::new(10, 1024);

    codec.push(&header).expect("push oversized header");
    let err = codec
        .next_frame()
        .expect_err("oversized header should be rejected before payload body");

    assert_eq!(err, KtpStreamCodecError::Ktp(KtpError::PayloadTooLarge(11)));
}

fn session_data(session_id: u64, payload: &[u8]) -> KtpFrame {
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
cargo test --test ktp_transport -- --nocapture
```

Expected: FAIL because `ktp_transport` is not exported.

- [ ] **Step 3: Implement minimal stream codec**

Create `src/ktp_transport.rs`:

```rust
use crate::ktp::{decode_frame, KtpError, KtpFrame, KTP_HEADER_LEN, KTP_MAX_PAYLOAD_LEN};
use std::error::Error;
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KtpStreamCodecError {
    Ktp(KtpError),
    BufferLimit { attempted: usize, limit: usize },
}

impl fmt::Display for KtpStreamCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ktp(error) => write!(f, "{error}"),
            Self::BufferLimit { attempted, limit } => {
                write!(f, "KTP stream buffer limit exceeded: attempted {attempted}, limit {limit}")
            }
        }
    }
}

impl Error for KtpStreamCodecError {}

impl From<KtpError> for KtpStreamCodecError {
    fn from(error: KtpError) -> Self {
        Self::Ktp(error)
    }
}

#[derive(Clone, Debug)]
pub struct KtpStreamCodec {
    buffer: Vec<u8>,
    max_payload_len: usize,
    max_buffer_len: usize,
}

impl KtpStreamCodec {
    pub fn new(max_payload_len: usize, max_buffer_len: usize) -> Self {
        Self {
            buffer: Vec::new(),
            max_payload_len: max_payload_len.min(KTP_MAX_PAYLOAD_LEN),
            max_buffer_len,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<(), KtpStreamCodecError> {
        let attempted = self.buffer.len().saturating_add(chunk.len());
        if attempted > self.max_buffer_len {
            return Err(KtpStreamCodecError::BufferLimit {
                attempted,
                limit: self.max_buffer_len,
            });
        }
        self.buffer.extend_from_slice(chunk);
        Ok(())
    }

    pub fn next_frame(&mut self) -> Result<Option<KtpFrame>, KtpStreamCodecError> {
        if self.buffer.len() < KTP_HEADER_LEN {
            return Ok(None);
        }
        let payload_len = u32::from_be_bytes(
            self.buffer[16..20]
                .try_into()
                .expect("KTP payload length slice is present"),
        ) as usize;
        if payload_len > self.max_payload_len {
            return Err(KtpError::PayloadTooLarge(payload_len).into());
        }
        let frame_len = KTP_HEADER_LEN + payload_len;
        if frame_len > self.max_buffer_len {
            return Err(KtpStreamCodecError::BufferLimit {
                attempted: frame_len,
                limit: self.max_buffer_len,
            });
        }
        if self.buffer.len() < frame_len {
            return Ok(None);
        }
        let frame = decode_frame(&self.buffer[..frame_len], self.max_payload_len)?;
        self.buffer.drain(..frame_len);
        Ok(Some(frame))
    }
}
```

Modify `src/lib.rs`:

```rust
pub mod ktp_transport;
```

- [ ] **Step 4: Run stream codec tests**

Run:

```powershell
cargo test --test ktp_transport -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src/lib.rs src/ktp_transport.rs tests/ktp_transport.rs docs/superpowers/plans/2026-06-17-ktp-stream-codec.md
git commit -m "feat: add ktp stream codec"
```
