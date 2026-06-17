# KTP Crypto Record Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a standalone authenticated-encryption record layer for KTP stream transport without changing the current WebSocket relay path.

**Architecture:** Keep existing KTP frames unchanged. Encode a KTP frame with the existing `ktp::encode_frame`, encrypt those bytes into a compact KTP crypto record, and decrypt records back into KTP frames before handing them to `KtpStreamCodec`. Use mature ChaCha20-Poly1305 AEAD for confidentiality and authentication; the self-developed part is record framing, associated data, sequence/nonce management, and bounded streaming decode.

**Tech Stack:** Rust 2021, `chacha20poly1305` AEAD, existing `ktp` module, existing `ktp_transport` stream codec, Cargo integration tests.

---

## File Structure

- Modify `Cargo.toml`
  - Add `chacha20poly1305 = "0.10"` for AEAD.
- Modify `src/ktp_transport.rs`
  - Add `KtpCryptoKey`, `KtpCryptoDirection`, `KtpCryptoSeal`, `KtpCryptoOpen`, `KtpCryptoRecordCodec`, and `KtpCryptoError`.
- Modify `tests/ktp_transport.rs`
  - Add crypto record round-trip, tamper rejection, nonce sequence increment, and split encrypted record decode tests.

## Task 1: Crypto Record Round Trip

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/ktp_transport.rs`
- Modify: `tests/ktp_transport.rs`

- [ ] **Step 1: Write failing crypto tests**

Append to `tests/ktp_transport.rs`:

```rust
use kelicloud_agent_rs::ktp_transport::{
    KtpCryptoDirection, KtpCryptoKey, KtpCryptoOpen, KtpCryptoRecordCodec, KtpCryptoSeal,
};

#[test]
fn crypto_record_round_trips_ktp_frame_and_hides_plaintext() {
    let key = test_crypto_key();
    let frame = session_data(700, b"rdp payload bytes");
    let mut seal = KtpCryptoSeal::new(key.clone(), KtpCryptoDirection::ClientToRelay);
    let mut open = KtpCryptoOpen::new(key, KtpCryptoDirection::ClientToRelay, KTP_MAX_PAYLOAD_LEN);

    let record = seal.seal_frame(&frame).expect("seal frame");
    assert!(!record.windows(b"rdp payload bytes".len()).any(|window| window == b"rdp payload bytes"));

    let decoded = open.open_record(&record).expect("open record");
    assert_eq!(decoded, frame);
}

#[test]
fn crypto_record_rejects_tampered_ciphertext() {
    let key = test_crypto_key();
    let frame = session_data(701, b"secret");
    let mut seal = KtpCryptoSeal::new(key.clone(), KtpCryptoDirection::ClientToRelay);
    let mut open = KtpCryptoOpen::new(key, KtpCryptoDirection::ClientToRelay, KTP_MAX_PAYLOAD_LEN);
    let mut record = seal.seal_frame(&frame).expect("seal frame");
    let last = record.len() - 1;
    record[last] ^= 0x55;

    let err = open.open_record(&record).expect_err("tampered record should fail auth");

    assert_eq!(err.code(), "auth_failed");
}

#[test]
fn crypto_record_codec_decodes_split_encrypted_records() {
    let key = test_crypto_key();
    let frame = session_data(702, b"chunked");
    let mut seal = KtpCryptoSeal::new(key.clone(), KtpCryptoDirection::RelayToClient);
    let record = seal.seal_frame(&frame).expect("seal frame");
    let mut codec =
        KtpCryptoRecordCodec::new(key, KtpCryptoDirection::RelayToClient, KTP_MAX_PAYLOAD_LEN, 1024 * 1024);

    codec.push(&record[..5]).expect("push first chunk");
    assert_eq!(codec.next_frame().expect("decode first chunk"), None);
    codec.push(&record[5..]).expect("push second chunk");

    assert_eq!(codec.next_frame().expect("decode frame"), Some(frame));
    assert_eq!(codec.next_frame().expect("decode empty"), None);
}

fn test_crypto_key() -> KtpCryptoKey {
    KtpCryptoKey::from_bytes([7u8; 32])
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test --test ktp_transport crypto_record -- --nocapture
```

Expected: FAIL because crypto record types do not exist.

- [ ] **Step 3: Add dependency and minimal AEAD record layer**

Add to `Cargo.toml`:

```toml
chacha20poly1305 = "0.10"
```

Modify `src/ktp_transport.rs` with:

```rust
use crate::ktp::{decode_frame, encode_frame};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};

pub const KTP_CRYPTO_MAGIC: &[u8; 4] = b"KTE1";
pub const KTP_CRYPTO_HEADER_LEN: usize = 24;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KtpCryptoDirection {
    ClientToRelay,
    RelayToClient,
}

impl KtpCryptoDirection {
    fn id(self) -> u8 {
        match self {
            Self::ClientToRelay => 1,
            Self::RelayToClient => 2,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KtpCryptoKey([u8; 32]);

impl KtpCryptoKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KtpCryptoError {
    code: &'static str,
    message: String,
}

impl KtpCryptoError {
    pub fn code(&self) -> &'static str {
        self.code
    }
}
```

Then add `KtpCryptoSeal`, `KtpCryptoOpen`, and `KtpCryptoRecordCodec`:

- Record header bytes:
  - `0..4`: `KTE1`
  - `4`: version `1`
  - `5`: direction id
  - `6..14`: sequence `u64` big endian
  - `14..18`: ciphertext length `u32` big endian
  - `18..24`: reserved zero bytes
- Nonce is 12 bytes: direction id, three zero bytes, then sequence `u64` big endian.
- Associated data is exactly the 24-byte record header.
- `seal_frame` encodes a KTP frame, encrypts it, prefixes the record header, and increments sequence.
- `open_record` validates header, decrypts ciphertext, and calls `decode_frame` on plaintext.
- `KtpCryptoRecordCodec` buffers split TCP chunks, validates record length before buffering too far, and returns decrypted frames.

- [ ] **Step 4: Run crypto tests**

Run:

```powershell
cargo test --test ktp_transport crypto_record -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Run regression tests**

Run:

```powershell
cargo test --test ktp --test ktp_transport --test tunnel_async_runtime -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```powershell
git add Cargo.toml Cargo.lock src/ktp_transport.rs tests/ktp_transport.rs docs/superpowers/plans/2026-06-17-ktp-crypto-record.md
git commit -m "feat: add ktp crypto record layer"
```
