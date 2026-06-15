# Tunnel Forwarding Phase 3 Skeleton Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the KTP data-plane skeleton across `kelicloud-agent-rs` and `kelicloud` without opening TCP listeners, dialing targets, or relaying payload bytes.

**Architecture:** The first data carrier is `KTP over WebSocket Binary over HTTPS`. Rust gets a KTP frame codec, data URL/config, and a non-fatal data runtime skeleton; Go gets a matching KTP codec, persistent data readiness state, and `/api/clients/tunnel/data` handshake/ready endpoint. The skeleton only proves protocol, authentication, readiness, and isolation from existing report/control loops.

**Tech Stack:** Rust + serde + tungstenite in `kelicloud-agent-rs`; Go + Gin + Gorilla WebSocket + GORM in `kelicloud`; existing token auth, user feature gates, and tunnel control state.

---

## Scope

This plan implements only the Phase 3 skeleton:

- KTP v1 frame encode/decode.
- WebSocket binary data URL helpers.
- Agent data runtime skeleton gated by `AGENT_TUNNEL_DATA_ENABLED`.
- Backend data endpoint skeleton at `/api/clients/tunnel/data`.
- Data-plane readiness state and tests.

This plan does not implement:

- TCP ingress listeners.
- Target dials.
- `SESSION_DATA` forwarding.
- Backend relay pairing.
- Flow-control runtime.
- raw TLS carrier.
- RDP smoke tests.

## File Structure

Agent repo: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs`

- Create `src/ktp.rs`
  - Owns KTP frame constants, frame type enum, leg enum, frame struct, encode/decode, and strict parse errors.
- Create `tests/ktp.rs`
  - Covers KTP encode/decode and parser rejection behavior.
- Modify `src/lib.rs`
  - Exports `ktp` and `tunnel_data`.
- Modify `src/protocol.rs`
  - Adds `build_tunnel_data_ws_url`.
- Modify `src/config.rs`
  - Adds `tunnel_data_enabled`, default `false`, env `AGENT_TUNNEL_DATA_ENABLED`.
- Modify `tests/config.rs`
  - Covers default-disabled and env-enabled data skeleton config.
- Modify `tests/protocol.rs`
  - Covers data URL helper.
- Create `src/tunnel_data.rs`
  - Owns data runtime message skeleton, fakeable transport traits, one-shot handshake/ready loop, and startup line redaction.
- Create `tests/tunnel_data.rs`
  - Tests skeleton sends `HELLO` and `READY`, rejects non-fatal endpoint errors, and never plans listeners.
- Modify `src/main.rs`
  - Starts data skeleton only when explicitly enabled.
- Modify handwritten `AgentConfig` test constructors
  - Adds `tunnel_data_enabled: false`.

Backend repo: `C:\Users\Administrator\Documents\tanzhen\kelicloud`

- Create `api/client/tunnel_data_protocol.go`
  - Owns backend KTP frame constants, frame parsing, encoding, payload helpers, and errors.
- Create `api/client/tunnel_data_protocol_test.go`
  - Mirrors Rust protocol tests for Go.
- Modify `database/models/tunnel.go`
  - Adds `ClientTunnelDataState`.
- Modify `database/dbcore/dbcore.go`
  - Migrates `ClientTunnelDataState`.
- Create `database/tunnel/data.go`
  - Owns data readiness upsert/disconnect helpers.
- Create `database/tunnel/data_test.go`
  - Tests data state migration and readiness updates.
- Create `api/client/tunnel_data.go`
  - Owns `/api/clients/tunnel/data` skeleton handler.
- Create `api/client/tunnel_data_test.go`
  - Tests helper parsing, feature/capability checks where possible, and binary frame responses.
- Modify `cmd/server.go`
  - Registers `tokenAuthrized.GET("/tunnel/data", client.WebSocketTunnelData)`.

---

### Task 1: Rust KTP Frame Codec

**Files:**
- Create: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\src\ktp.rs`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\src\lib.rs`
- Test: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\tests\ktp.rs`

- [ ] **Step 1: Write failing KTP codec tests**

Create `tests/ktp.rs`:

```rust
use kelicloud_agent_rs::ktp::{
    decode_frame, encode_frame, FrameLeg, FrameType, KtpError, KtpFrame, KTP_HEADER_LEN,
    KTP_MAX_PAYLOAD_LEN, KTP_VERSION,
};

#[test]
fn ktp_encodes_and_decodes_hello_frame() {
    let frame = KtpFrame {
        frame_type: FrameType::Hello,
        leg: FrameLeg::Connection,
        flags: 0,
        session_id: 0,
        payload: b"agent-a".to_vec(),
    };

    let encoded = encode_frame(&frame).unwrap();

    assert_eq!(encoded.len(), KTP_HEADER_LEN + frame.payload.len());
    assert_eq!(&encoded[0..4], b"KTP1");
    assert_eq!(encoded[4], KTP_VERSION);

    let decoded = decode_frame(&encoded, KTP_MAX_PAYLOAD_LEN).unwrap();
    assert_eq!(decoded, frame);
}

#[test]
fn ktp_preserves_session_data_payload_and_leg() {
    let frame = KtpFrame {
        frame_type: FrameType::SessionData,
        leg: FrameLeg::Ingress,
        flags: 1,
        session_id: 42,
        payload: vec![0, 1, 2, 3, 255],
    };

    let decoded = decode_frame(&encode_frame(&frame).unwrap(), KTP_MAX_PAYLOAD_LEN).unwrap();

    assert_eq!(decoded.frame_type, FrameType::SessionData);
    assert_eq!(decoded.leg, FrameLeg::Ingress);
    assert_eq!(decoded.flags, 1);
    assert_eq!(decoded.session_id, 42);
    assert_eq!(decoded.payload, vec![0, 1, 2, 3, 255]);
}

#[test]
fn ktp_rejects_wrong_magic() {
    let mut encoded = encode_frame(&KtpFrame::connection(FrameType::Ping, Vec::new())).unwrap();
    encoded[0] = b'X';

    assert_eq!(
        decode_frame(&encoded, KTP_MAX_PAYLOAD_LEN).unwrap_err(),
        KtpError::WrongMagic
    );
}

#[test]
fn ktp_rejects_unsupported_version() {
    let mut encoded = encode_frame(&KtpFrame::connection(FrameType::Ping, Vec::new())).unwrap();
    encoded[4] = 2;

    assert_eq!(
        decode_frame(&encoded, KTP_MAX_PAYLOAD_LEN).unwrap_err(),
        KtpError::UnsupportedVersion(2)
    );
}

#[test]
fn ktp_rejects_invalid_connection_leg_for_session_frame() {
    let frame = KtpFrame {
        frame_type: FrameType::SessionOpen,
        leg: FrameLeg::Connection,
        flags: 0,
        session_id: 7,
        payload: Vec::new(),
    };

    assert_eq!(encode_frame(&frame).unwrap_err(), KtpError::InvalidLeg(0));
}

#[test]
fn ktp_rejects_truncated_header_and_payload() {
    assert_eq!(
        decode_frame(&[0; KTP_HEADER_LEN - 1], KTP_MAX_PAYLOAD_LEN).unwrap_err(),
        KtpError::TruncatedHeader
    );

    let mut encoded = encode_frame(&KtpFrame::connection(FrameType::Ping, b"abc".to_vec())).unwrap();
    encoded.truncate(KTP_HEADER_LEN + 2);

    assert_eq!(
        decode_frame(&encoded, KTP_MAX_PAYLOAD_LEN).unwrap_err(),
        KtpError::TruncatedPayload
    );
}

#[test]
fn ktp_rejects_payload_above_limit() {
    let encoded = encode_frame(&KtpFrame::connection(FrameType::Ping, b"abc".to_vec())).unwrap();

    assert_eq!(
        decode_frame(&encoded, 2).unwrap_err(),
        KtpError::PayloadTooLarge(3)
    );
}
```

- [ ] **Step 2: Run failing KTP tests**

Run in `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs`:

```powershell
cargo test --test ktp
```

Expected: FAIL with `could not find ktp in kelicloud_agent_rs`.

- [ ] **Step 3: Implement the Rust KTP codec**

Create `src/ktp.rs`:

```rust
use std::error::Error;
use std::fmt;

pub const KTP_MAGIC: &[u8; 4] = b"KTP1";
pub const KTP_VERSION: u8 = 1;
pub const KTP_HEADER_LEN: usize = 24;
pub const KTP_MAX_PAYLOAD_LEN: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    Hello = 0x01,
    HelloAck = 0x02,
    Ready = 0x03,
    SessionOpen = 0x10,
    SessionAccept = 0x11,
    SessionData = 0x12,
    SessionWindow = 0x13,
    SessionClose = 0x14,
    SessionError = 0x15,
    Ping = 0x20,
    Pong = 0x21,
    Stats = 0x30,
}

impl FrameType {
    fn from_u8(value: u8) -> Result<Self, KtpError> {
        match value {
            0x01 => Ok(Self::Hello),
            0x02 => Ok(Self::HelloAck),
            0x03 => Ok(Self::Ready),
            0x10 => Ok(Self::SessionOpen),
            0x11 => Ok(Self::SessionAccept),
            0x12 => Ok(Self::SessionData),
            0x13 => Ok(Self::SessionWindow),
            0x14 => Ok(Self::SessionClose),
            0x15 => Ok(Self::SessionError),
            0x20 => Ok(Self::Ping),
            0x21 => Ok(Self::Pong),
            0x30 => Ok(Self::Stats),
            other => Err(KtpError::UnknownFrameType(other)),
        }
    }

    fn is_connection_level(self) -> bool {
        matches!(
            self,
            Self::Hello | Self::HelloAck | Self::Ready | Self::Ping | Self::Pong | Self::Stats
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameLeg {
    Connection = 0,
    Ingress = 1,
    Egress = 2,
}

impl FrameLeg {
    fn from_u8(value: u8) -> Result<Self, KtpError> {
        match value {
            0 => Ok(Self::Connection),
            1 => Ok(Self::Ingress),
            2 => Ok(Self::Egress),
            other => Err(KtpError::InvalidLeg(other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KtpFrame {
    pub frame_type: FrameType,
    pub leg: FrameLeg,
    pub flags: u8,
    pub session_id: u64,
    pub payload: Vec<u8>,
}

impl KtpFrame {
    pub fn connection(frame_type: FrameType, payload: Vec<u8>) -> Self {
        Self {
            frame_type,
            leg: FrameLeg::Connection,
            flags: 0,
            session_id: 0,
            payload,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KtpError {
    WrongMagic,
    UnsupportedVersion(u8),
    UnknownFrameType(u8),
    InvalidLeg(u8),
    InvalidSessionId,
    TruncatedHeader,
    TruncatedPayload,
    PayloadTooLarge(usize),
    ReservedNonZero(u32),
}

impl fmt::Display for KtpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongMagic => write!(f, "wrong KTP magic"),
            Self::UnsupportedVersion(version) => write!(f, "unsupported KTP version: {version}"),
            Self::UnknownFrameType(kind) => write!(f, "unknown KTP frame type: {kind}"),
            Self::InvalidLeg(leg) => write!(f, "invalid KTP frame leg: {leg}"),
            Self::InvalidSessionId => write!(f, "invalid KTP session id"),
            Self::TruncatedHeader => write!(f, "truncated KTP header"),
            Self::TruncatedPayload => write!(f, "truncated KTP payload"),
            Self::PayloadTooLarge(size) => write!(f, "KTP payload too large: {size}"),
            Self::ReservedNonZero(value) => write!(f, "KTP reserved field must be zero: {value}"),
        }
    }
}

impl Error for KtpError {}

pub fn encode_frame(frame: &KtpFrame) -> Result<Vec<u8>, KtpError> {
    validate_frame(frame)?;
    let payload_len = frame.payload.len();
    if payload_len > KTP_MAX_PAYLOAD_LEN {
        return Err(KtpError::PayloadTooLarge(payload_len));
    }

    let mut out = Vec::with_capacity(KTP_HEADER_LEN + payload_len);
    out.extend_from_slice(KTP_MAGIC);
    out.push(KTP_VERSION);
    out.push(frame.frame_type as u8);
    out.push(frame.leg as u8);
    out.push(frame.flags);
    out.extend_from_slice(&frame.session_id.to_be_bytes());
    out.extend_from_slice(&(payload_len as u32).to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&frame.payload);
    Ok(out)
}

pub fn decode_frame(bytes: &[u8], max_payload_len: usize) -> Result<KtpFrame, KtpError> {
    if bytes.len() < KTP_HEADER_LEN {
        return Err(KtpError::TruncatedHeader);
    }
    if &bytes[0..4] != KTP_MAGIC {
        return Err(KtpError::WrongMagic);
    }
    let version = bytes[4];
    if version != KTP_VERSION {
        return Err(KtpError::UnsupportedVersion(version));
    }
    let frame_type = FrameType::from_u8(bytes[5])?;
    let leg = FrameLeg::from_u8(bytes[6])?;
    let flags = bytes[7];
    let session_id = u64::from_be_bytes(bytes[8..16].try_into().expect("session id slice length"));
    let payload_len =
        u32::from_be_bytes(bytes[16..20].try_into().expect("payload len slice length")) as usize;
    let reserved = u32::from_be_bytes(bytes[20..24].try_into().expect("reserved slice length"));
    if reserved != 0 {
        return Err(KtpError::ReservedNonZero(reserved));
    }
    if payload_len > max_payload_len {
        return Err(KtpError::PayloadTooLarge(payload_len));
    }
    if bytes.len() < KTP_HEADER_LEN + payload_len {
        return Err(KtpError::TruncatedPayload);
    }
    let frame = KtpFrame {
        frame_type,
        leg,
        flags,
        session_id,
        payload: bytes[KTP_HEADER_LEN..KTP_HEADER_LEN + payload_len].to_vec(),
    };
    validate_frame(&frame)?;
    Ok(frame)
}

fn validate_frame(frame: &KtpFrame) -> Result<(), KtpError> {
    if frame.frame_type.is_connection_level() {
        if frame.leg != FrameLeg::Connection {
            return Err(KtpError::InvalidLeg(frame.leg as u8));
        }
        if frame.session_id != 0 {
            return Err(KtpError::InvalidSessionId);
        }
        return Ok(());
    }
    if frame.leg == FrameLeg::Connection {
        return Err(KtpError::InvalidLeg(frame.leg as u8));
    }
    if frame.session_id == 0 {
        return Err(KtpError::InvalidSessionId);
    }
    Ok(())
}
```

Modify `src/lib.rs`:

```rust
pub mod ktp;
```

- [ ] **Step 4: Run KTP tests**

Run:

```powershell
cargo test --test ktp
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src/ktp.rs src/lib.rs tests/ktp.rs
git commit -m "Add KTP frame codec"
```

---

### Task 2: Rust Data URL And Config Gate

**Files:**
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\src\protocol.rs`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\src\config.rs`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\tests\protocol.rs`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\tests\config.rs`
- Modify: Rust tests with manual `AgentConfig` literals:
  - `tests\runtime.rs`
  - `tests\auto_discovery.rs`
  - `tests\task.rs`
  - `tests\terminal.rs`
  - `tests\transport.rs`

- [ ] **Step 1: Write failing data URL/config tests**

Add to `tests/protocol.rs`:

```rust
#[test]
fn tunnel_data_ws_url_adds_token() {
    let url = build_tunnel_data_ws_url("https://panel.example.com/base/", "tok").unwrap();

    assert_eq!(
        url,
        "wss://panel.example.com/base/api/clients/tunnel/data?token=tok"
    );
}
```

Update the protocol import:

```rust
use kelicloud_agent_rs::protocol::{
    build_report_ws_url, build_terminal_ws_url, build_tunnel_control_ws_url,
    build_tunnel_data_ws_url, parse_backend_message, BackendMessage,
};
```

Add to `tests/config.rs`:

```rust
#[test]
fn config_disables_tunnel_data_by_default() {
    let config = AgentConfig::from_args_and_env(["kelicloud-agent-rs"], |key| match key {
        "AGENT_ENDPOINT" => Some("https://env.example.com".to_string()),
        "AGENT_TOKEN" => Some("env-token".to_string()),
        _ => None,
    })
    .unwrap();

    assert!(!config.tunnel_data_enabled);
}

#[test]
fn config_can_enable_tunnel_data_from_environment() {
    let config = AgentConfig::from_args_and_env(["kelicloud-agent-rs"], |key| match key {
        "AGENT_ENDPOINT" => Some("https://env.example.com".to_string()),
        "AGENT_TOKEN" => Some("env-token".to_string()),
        "AGENT_TUNNEL_DATA_ENABLED" => Some("true".to_string()),
        _ => None,
    })
    .unwrap();

    assert!(config.tunnel_data_enabled);
}
```

- [ ] **Step 2: Run failing data URL/config tests**

Run:

```powershell
cargo test --test protocol tunnel_data_ws_url
cargo test --test config tunnel_data
```

Expected: FAIL with missing `build_tunnel_data_ws_url` and missing `tunnel_data_enabled`.

- [ ] **Step 3: Implement URL helper and config field**

Modify `src/protocol.rs`:

```rust
pub fn build_tunnel_data_ws_url(endpoint: &str, token: &str) -> Result<String, ProtocolError> {
    let token = require_non_empty(token, ProtocolError::EmptyToken)?;
    build_ws_url(endpoint, "/api/clients/tunnel/data", &[("token", token)])
}
```

Modify `src/config.rs`:

```rust
pub struct AgentConfig {
    pub tunnel_data_enabled: bool,
}
```

In `from_args_and_env`, add:

```rust
let mut tunnel_data_enabled = env_lookup("AGENT_TUNNEL_DATA_ENABLED")
    .as_deref()
    .map(parse_bool)
    .unwrap_or(false);
```

In the env override section, add:

```rust
apply_bool_env(&env_lookup, "AGENT_TUNNEL_DATA_ENABLED", &mut tunnel_data_enabled);
```

Add this helper near `apply_bool_true_env`:

```rust
fn apply_bool_env<F>(env_lookup: &F, key: &str, target: &mut bool)
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(value) = env_lookup(key) {
        *target = parse_bool(&value);
    }
}
```

Add to `FileConfig`:

```rust
tunnel_data_enabled: Option<bool>,
```

Apply file config:

```rust
if let Some(value) = file_config.tunnel_data_enabled {
    tunnel_data_enabled = value;
}
```

Return field:

```rust
tunnel_data_enabled,
```

In every manual `AgentConfig` literal in tests, add:

```rust
tunnel_data_enabled: false,
```

- [ ] **Step 4: Run config/protocol tests**

Run:

```powershell
cargo test --test protocol tunnel_data_ws_url
cargo test --test config tunnel_data
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src/protocol.rs src/config.rs tests/protocol.rs tests/config.rs tests/runtime.rs tests/auto_discovery.rs tests/task.rs tests/terminal.rs tests/transport.rs
git commit -m "Add tunnel data config and URL"
```

---

### Task 3: Rust Tunnel Data Runtime Skeleton

**Files:**
- Create: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\src\tunnel_data.rs`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\src\lib.rs`
- Test: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\tests\tunnel_data.rs`

- [ ] **Step 1: Write failing tunnel data runtime tests**

Create `tests/tunnel_data.rs`:

```rust
use kelicloud_agent_rs::ktp::{decode_frame, FrameType, KTP_MAX_PAYLOAD_LEN};
use kelicloud_agent_rs::transport::{HeaderPair, TransportError};
use kelicloud_agent_rs::tunnel_data::{
    run_tunnel_data_once, tunnel_data_startup_line, TunnelDataReadyState, TunnelDataSocket,
    TunnelDataTransport,
};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn tunnel_data_once_sends_hello_and_ready_without_listener_plan() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut transport = FakeTunnelDataTransport::new(events.clone());
    let ready = TunnelDataReadyState {
        revision: "rev-a".to_string(),
        ingress_rule_ids: vec![7],
        egress_rule_ids: vec![9],
        failed_rules: Vec::new(),
    };

    run_tunnel_data_once(
        "wss://panel.example.com/api/clients/tunnel/data?token=secret",
        &[],
        "node-a",
        "0.1.0",
        &ready,
        &mut transport,
    )
    .unwrap();

    assert_eq!(
        events.borrow()[0],
        "connect:wss://panel.example.com/api/clients/tunnel/data?token=secret"
    );
    let frames = events
        .borrow()
        .iter()
        .filter_map(|event| event.strip_prefix("frame:"))
        .map(hex_to_bytes)
        .collect::<Vec<_>>();
    assert_eq!(frames.len(), 2);
    assert_eq!(
        decode_frame(&frames[0], KTP_MAX_PAYLOAD_LEN).unwrap().frame_type,
        FrameType::Hello
    );
    assert_eq!(
        decode_frame(&frames[1], KTP_MAX_PAYLOAD_LEN).unwrap().frame_type,
        FrameType::Ready
    );
}

#[test]
fn tunnel_data_unsupported_endpoint_is_non_fatal() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut transport = FakeTunnelDataTransport::new(events)
        .with_connect_error(TransportError::RequestFailed("status=404".to_string()));
    let ready = TunnelDataReadyState::empty("rev-a");

    let result = run_tunnel_data_once(
        "wss://panel.example.com/api/clients/tunnel/data?token=secret",
        &[],
        "node-a",
        "0.1.0",
        &ready,
        &mut transport,
    );

    assert!(result.is_ok());
}

#[test]
fn tunnel_data_startup_line_redacts_token() {
    let line = tunnel_data_startup_line(
        "wss://panel.example.com/api/clients/tunnel/data?token=secret",
        true,
    );

    assert_eq!(
        line,
        "tunnel data: enabled url=wss://panel.example.com/api/clients/tunnel/data?token=redacted"
    );
}

struct FakeTunnelDataTransport {
    events: Rc<RefCell<Vec<String>>>,
    connect_error: Option<TransportError>,
}

impl FakeTunnelDataTransport {
    fn new(events: Rc<RefCell<Vec<String>>>) -> Self {
        Self {
            events,
            connect_error: None,
        }
    }

    fn with_connect_error(mut self, error: TransportError) -> Self {
        self.connect_error = Some(error);
        self
    }
}

impl TunnelDataTransport for FakeTunnelDataTransport {
    type Socket = FakeTunnelDataSocket;

    fn connect_tunnel_data(
        &mut self,
        url: &str,
        _headers: &[HeaderPair],
    ) -> Result<Self::Socket, TransportError> {
        self.events.borrow_mut().push(format!("connect:{url}"));
        if let Some(error) = self.connect_error.take() {
            return Err(error);
        }
        Ok(FakeTunnelDataSocket {
            events: self.events.clone(),
        })
    }
}

struct FakeTunnelDataSocket {
    events: Rc<RefCell<Vec<String>>>,
}

impl TunnelDataSocket for FakeTunnelDataSocket {
    fn send_frame(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        self.events
            .borrow_mut()
            .push(format!("frame:{}", bytes_to_hex(frame)));
        Ok(())
    }
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex_to_bytes(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).unwrap();
            u8::from_str_radix(text, 16).unwrap()
        })
        .collect()
}
```

- [ ] **Step 2: Run failing tunnel data tests**

Run:

```powershell
cargo test --test tunnel_data
```

Expected: FAIL with `could not find tunnel_data in kelicloud_agent_rs`.

- [ ] **Step 3: Implement data runtime skeleton**

Create `src/tunnel_data.rs`:

```rust
use crate::ktp::{encode_frame, FrameType, KtpFrame};
use crate::transport::{HeaderPair, TransportError};

#[derive(Debug, Clone, PartialEq, Eq)]
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelDataRuleFailure {
    pub rule_id: u64,
    pub status: String,
    pub error: String,
}

pub trait TunnelDataSocket {
    fn send_frame(&mut self, frame: &[u8]) -> Result<(), TransportError>;
}

pub trait TunnelDataTransport {
    type Socket: TunnelDataSocket;

    fn connect_tunnel_data(
        &mut self,
        url: &str,
        headers: &[HeaderPair],
    ) -> Result<Self::Socket, TransportError>;
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
        Err(error) if is_non_fatal_tunnel_data_error(&error) => return Ok(()),
        Err(error) => return Err(error),
    };
    socket.send_frame(&encode_frame(&KtpFrame::connection(
        FrameType::Hello,
        encode_hello_payload(agent_id_hint, agent_version, &ready.revision),
    ))
    .map_err(|error| TransportError::RequestFailed(error.to_string()))?)?;
    socket.send_frame(&encode_frame(&KtpFrame::connection(
        FrameType::Ready,
        encode_ready_payload(ready),
    ))
    .map_err(|error| TransportError::RequestFailed(error.to_string()))?)?;
    Ok(())
}

fn is_non_fatal_tunnel_data_error(error: &TransportError) -> bool {
    match error {
        TransportError::InvalidClientToken { .. } => false,
        TransportError::EmptyEndpoint
        | TransportError::EmptyToken
        | TransportError::UnsupportedScheme(_) => false,
        TransportError::RequestFailed(message) => {
            let lower = message.to_ascii_lowercase();
            lower.contains("404") || lower.contains("403") || lower.contains("feature_disabled")
        }
        TransportError::SocketClosed => true,
    }
}

fn encode_hello_payload(agent_id_hint: &str, agent_version: &str, revision: &str) -> Vec<u8> {
    let mut out = Vec::new();
    write_string(&mut out, agent_id_hint);
    write_string(&mut out, agent_version);
    write_string(&mut out, revision);
    write_string_list(&mut out, &["tcp", "multiplex", "flow_control", "stats"]);
    out
}

fn encode_ready_payload(ready: &TunnelDataReadyState) -> Vec<u8> {
    let mut out = Vec::new();
    write_string(&mut out, &ready.revision);
    write_u64_list(&mut out, &ready.ingress_rule_ids);
    write_u64_list(&mut out, &ready.egress_rule_ids);
    out.extend_from_slice(&(ready.failed_rules.len() as u16).to_be_bytes());
    for failure in &ready.failed_rules {
        out.extend_from_slice(&failure.rule_id.to_be_bytes());
        write_string(&mut out, &failure.status);
        write_string(&mut out, &failure.error);
    }
    out
}

fn write_string(out: &mut Vec<u8>, value: &str) {
    let bytes = value.trim().as_bytes();
    out.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
    out.extend_from_slice(bytes);
}

fn write_string_list(out: &mut Vec<u8>, values: &[&str]) {
    out.extend_from_slice(&(values.len() as u16).to_be_bytes());
    for value in values {
        write_string(out, value);
    }
}

fn write_u64_list(out: &mut Vec<u8>, values: &[u64]) {
    out.extend_from_slice(&(values.len() as u16).to_be_bytes());
    for value in values {
        out.extend_from_slice(&value.to_be_bytes());
    }
}

pub fn tunnel_data_startup_line(url: &str, enabled: bool) -> String {
    if !enabled {
        return "tunnel data: disabled".to_string();
    }
    format!("tunnel data: enabled url={}", redact_token_in_url(url))
}

fn redact_token_in_url(url: &str) -> String {
    let Some((base, query)) = url.split_once('?') else {
        return url.to_string();
    };
    let query = query
        .split('&')
        .map(|part| {
            if part.split_once('=').is_some_and(|(key, _)| key == "token") {
                "token=redacted".to_string()
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("&");
    format!("{base}?{query}")
}
```

Modify `src/lib.rs`:

```rust
pub mod tunnel_data;
```

- [ ] **Step 4: Run tunnel data tests**

Run:

```powershell
cargo test --test tunnel_data
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src/tunnel_data.rs src/lib.rs tests/tunnel_data.rs
git commit -m "Add tunnel data runtime skeleton"
```

---

### Task 4: Rust Main Integration For Data Skeleton

**Files:**
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\src\main.rs`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\src\tunnel_data.rs`
- Test: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\tests\tunnel_data.rs`
- Test: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\tests\runtime.rs`

- [ ] **Step 1: Add startup line and default-disabled assertions**

Add to `tests\tunnel_data.rs`:

```rust
use kelicloud_agent_rs::tunnel_data::TungsteniteTunnelDataTransport;

#[test]
fn tunnel_data_startup_line_reports_disabled() {
    assert_eq!(tunnel_data_startup_line("", false), "tunnel data: disabled");
}

#[test]
fn tungstenite_tunnel_data_transport_can_be_constructed() {
    let _transport = TungsteniteTunnelDataTransport::new_with_custom_dns("8.8.8.8");
}
```

Add to `tests\runtime.rs` inside `startup_summary_redacts_token`:

```rust
assert!(!summary.contains("tunnel data"));
```

This keeps existing startup summary unchanged. The data skeleton prints its own
line from `main` only when `AGENT_TUNNEL_DATA_ENABLED=true`.

- [ ] **Step 2: Run tests before integration**

Run:

```powershell
cargo test --test tunnel_data tunnel_data_startup_line
cargo test --test tunnel_data tungstenite_tunnel_data_transport
cargo test startup_summary --test runtime
```

Expected:

- `tunnel_data_startup_line_reports_disabled` passes because Task 3 added the helper.
- `tungstenite_tunnel_data_transport_can_be_constructed` FAILS with missing `TungsteniteTunnelDataTransport`.
- runtime summary test passes because summary has no data line.

- [ ] **Step 3: Add real WebSocket transport skeleton**

Append to `src\tunnel_data.rs`:

```rust
use crate::transport::connect_websocket_request;
use std::net::TcpStream;
use tungstenite::client::IntoClientRequest;
use tungstenite::http::{HeaderName, HeaderValue};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

#[derive(Debug, Default, Clone)]
pub struct TungsteniteTunnelDataTransport {
    custom_dns: String,
}

impl TungsteniteTunnelDataTransport {
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
        Ok(TungsteniteTunnelDataSocket { socket })
    }
}

pub struct TungsteniteTunnelDataSocket {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
}

impl TunnelDataSocket for TungsteniteTunnelDataSocket {
    fn send_frame(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        self.socket
            .send(Message::Binary(frame.to_vec().into()))
            .map_err(|error| TransportError::RequestFailed(error.to_string()))
    }
}
```

- [ ] **Step 4: Start the data skeleton from main only when enabled**

Modify `src\main.rs` imports:

```rust
use kelicloud_agent_rs::protocol::{build_tunnel_control_ws_url, build_tunnel_data_ws_url};
use kelicloud_agent_rs::tunnel_data::{
    run_tunnel_data_once, tunnel_data_startup_line, TunnelDataReadyState,
    TungsteniteTunnelDataTransport,
};
```

After the tunnel control startup print block, add:

```rust
let tunnel_data_url = build_tunnel_data_ws_url(&config.endpoint, &shared_token.get()).ok();
if let Some(url) = tunnel_data_url.as_deref() {
    println!("{}", tunnel_data_startup_line(url, config.tunnel_data_enabled));
} else if config.tunnel_data_enabled {
    println!("tunnel data: enabled url=invalid");
} else {
    println!("{}", tunnel_data_startup_line("", false));
}
```

Before token recovery setup, add:

```rust
if config.tunnel_data_enabled {
    let tunnel_data_headers = access_headers(&config);
    let tunnel_data_endpoint = config.endpoint.clone();
    let tunnel_data_custom_dns = config.custom_dns.clone();
    let tunnel_data_agent_version = env!("CARGO_PKG_VERSION").to_string();
    let tunnel_data_shared_token = shared_token.clone();
    std::thread::spawn(move || {
        let ready = TunnelDataReadyState::empty("");
        loop {
            match build_tunnel_data_ws_url(&tunnel_data_endpoint, &tunnel_data_shared_token.get()) {
                Ok(url) => {
                    let mut transport =
                        TungsteniteTunnelDataTransport::new_with_custom_dns(&tunnel_data_custom_dns);
                    if let Err(error) = run_tunnel_data_once(
                        &url,
                        &tunnel_data_headers,
                        "",
                        &tunnel_data_agent_version,
                        &ready,
                        &mut transport,
                    ) {
                        eprintln!("tunnel data warning: {error}");
                    }
                }
                Err(error) => eprintln!("tunnel data warning: {error}"),
            }
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
    });
}
```

This skeleton sends an empty client id hint and empty readiness. The backend must identify the client from the already-authenticated token, not from the HELLO payload. A later implementation can add a server-assigned client id after the backend exposes it. It must not bind ports or relay bytes.

- [ ] **Step 5: Run Rust tests**

Run:

```powershell
cargo fmt
cargo test --test tunnel_data
cargo test startup_summary --test runtime
```

Expected: PASS.

- [ ] **Step 6: Commit**

```powershell
git add src/main.rs src/tunnel_data.rs tests/tunnel_data.rs tests/runtime.rs
git commit -m "Start tunnel data skeleton when enabled"
```

---

### Task 5: Go KTP Frame Codec

**Files:**
- Create: `C:\Users\Administrator\Documents\tanzhen\kelicloud\api\client\tunnel_data_protocol.go`
- Test: `C:\Users\Administrator\Documents\tanzhen\kelicloud\api\client\tunnel_data_protocol_test.go`

- [ ] **Step 1: Write failing Go KTP protocol tests**

Create `api/client/tunnel_data_protocol_test.go`:

```go
package client

import (
	"bytes"
	"testing"
)

func TestKTPEncodeDecodeHelloFrame(t *testing.T) {
	frame := ktpFrame{
		Type:      ktpFrameHello,
		Leg:       ktpLegConnection,
		SessionID: 0,
		Payload:   []byte("node-a"),
	}

	encoded, err := encodeKTPFrame(frame)
	if err != nil {
		t.Fatalf("encode frame: %v", err)
	}
	if len(encoded) != ktpHeaderLen+len(frame.Payload) {
		t.Fatalf("unexpected encoded length: %d", len(encoded))
	}
	if !bytes.Equal(encoded[:4], []byte("KTP1")) {
		t.Fatalf("unexpected magic: %q", encoded[:4])
	}

	decoded, err := decodeKTPFrame(encoded, ktpMaxPayloadLen)
	if err != nil {
		t.Fatalf("decode frame: %v", err)
	}
	if decoded.Type != frame.Type || decoded.Leg != frame.Leg || decoded.SessionID != 0 || string(decoded.Payload) != "node-a" {
		t.Fatalf("unexpected decoded frame: %+v", decoded)
	}
}

func TestKTPRejectsWrongMagicAndTruncatedPayload(t *testing.T) {
	encoded, err := encodeKTPFrame(ktpFrame{Type: ktpFramePing, Leg: ktpLegConnection})
	if err != nil {
		t.Fatalf("encode frame: %v", err)
	}
	encoded[0] = 'X'
	if _, err := decodeKTPFrame(encoded, ktpMaxPayloadLen); err == nil || err.Error() != "wrong KTP magic" {
		t.Fatalf("expected wrong magic, got %v", err)
	}

	encoded, err = encodeKTPFrame(ktpFrame{Type: ktpFramePing, Leg: ktpLegConnection, Payload: []byte("abc")})
	if err != nil {
		t.Fatalf("encode frame: %v", err)
	}
	encoded = encoded[:ktpHeaderLen+2]
	if _, err := decodeKTPFrame(encoded, ktpMaxPayloadLen); err == nil || err.Error() != "truncated KTP payload" {
		t.Fatalf("expected truncated payload, got %v", err)
	}
}

func TestKTPRejectsInvalidLegForSessionFrame(t *testing.T) {
	_, err := encodeKTPFrame(ktpFrame{
		Type:      ktpFrameSessionOpen,
		Leg:       ktpLegConnection,
		SessionID: 7,
	})
	if err == nil || err.Error() != "invalid KTP frame leg: 0" {
		t.Fatalf("expected invalid leg, got %v", err)
	}
}
```

- [ ] **Step 2: Run failing Go protocol tests**

Run in `C:\Users\Administrator\Documents\tanzhen\kelicloud`:

```powershell
go test ./api/client -run TestKTP -count=1
```

Expected: FAIL with undefined `ktpFrame`.

- [ ] **Step 3: Implement Go KTP protocol helpers**

Create `api/client/tunnel_data_protocol.go`:

```go
package client

import (
	"encoding/binary"
	"fmt"
)

const (
	ktpVersion       = byte(1)
	ktpHeaderLen     = 24
	ktpMaxPayloadLen = 1024 * 1024

	ktpFrameHello         = byte(0x01)
	ktpFrameHelloAck      = byte(0x02)
	ktpFrameReady         = byte(0x03)
	ktpFrameSessionOpen   = byte(0x10)
	ktpFrameSessionAccept = byte(0x11)
	ktpFrameSessionData   = byte(0x12)
	ktpFrameSessionWindow = byte(0x13)
	ktpFrameSessionClose  = byte(0x14)
	ktpFrameSessionError  = byte(0x15)
	ktpFramePing          = byte(0x20)
	ktpFramePong          = byte(0x21)
	ktpFrameStats         = byte(0x30)

	ktpLegConnection = byte(0)
	ktpLegIngress    = byte(1)
	ktpLegEgress     = byte(2)
)

type ktpFrame struct {
	Type      byte
	Leg       byte
	Flags     byte
	SessionID uint64
	Payload   []byte
}

func encodeKTPFrame(frame ktpFrame) ([]byte, error) {
	if err := validateKTPFrame(frame); err != nil {
		return nil, err
	}
	if len(frame.Payload) > ktpMaxPayloadLen {
		return nil, fmt.Errorf("KTP payload too large: %d", len(frame.Payload))
	}
	out := make([]byte, ktpHeaderLen+len(frame.Payload))
	copy(out[0:4], []byte("KTP1"))
	out[4] = ktpVersion
	out[5] = frame.Type
	out[6] = frame.Leg
	out[7] = frame.Flags
	binary.BigEndian.PutUint64(out[8:16], frame.SessionID)
	binary.BigEndian.PutUint32(out[16:20], uint32(len(frame.Payload)))
	binary.BigEndian.PutUint32(out[20:24], 0)
	copy(out[ktpHeaderLen:], frame.Payload)
	return out, nil
}

func decodeKTPFrame(bytes []byte, maxPayloadLen int) (ktpFrame, error) {
	if len(bytes) < ktpHeaderLen {
		return ktpFrame{}, fmt.Errorf("truncated KTP header")
	}
	if string(bytes[0:4]) != "KTP1" {
		return ktpFrame{}, fmt.Errorf("wrong KTP magic")
	}
	if bytes[4] != ktpVersion {
		return ktpFrame{}, fmt.Errorf("unsupported KTP version: %d", bytes[4])
	}
	frameType := bytes[5]
	if !isKnownKTPFrameType(frameType) {
		return ktpFrame{}, fmt.Errorf("unknown KTP frame type: %d", frameType)
	}
	payloadLen := int(binary.BigEndian.Uint32(bytes[16:20]))
	if payloadLen > maxPayloadLen {
		return ktpFrame{}, fmt.Errorf("KTP payload too large: %d", payloadLen)
	}
	if binary.BigEndian.Uint32(bytes[20:24]) != 0 {
		return ktpFrame{}, fmt.Errorf("KTP reserved field must be zero")
	}
	if len(bytes) < ktpHeaderLen+payloadLen {
		return ktpFrame{}, fmt.Errorf("truncated KTP payload")
	}
	frame := ktpFrame{
		Type:      frameType,
		Leg:       bytes[6],
		Flags:     bytes[7],
		SessionID: binary.BigEndian.Uint64(bytes[8:16]),
		Payload:   append([]byte(nil), bytes[ktpHeaderLen:ktpHeaderLen+payloadLen]...),
	}
	if err := validateKTPFrame(frame); err != nil {
		return ktpFrame{}, err
	}
	return frame, nil
}

func validateKTPFrame(frame ktpFrame) error {
	if !isKnownKTPLeg(frame.Leg) {
		return fmt.Errorf("invalid KTP frame leg: %d", frame.Leg)
	}
	if isConnectionLevelKTPFrame(frame.Type) {
		if frame.Leg != ktpLegConnection {
			return fmt.Errorf("invalid KTP frame leg: %d", frame.Leg)
		}
		if frame.SessionID != 0 {
			return fmt.Errorf("invalid KTP session id")
		}
		return nil
	}
	if frame.Leg == ktpLegConnection {
		return fmt.Errorf("invalid KTP frame leg: %d", frame.Leg)
	}
	if frame.SessionID == 0 {
		return fmt.Errorf("invalid KTP session id")
	}
	return nil
}

func isKnownKTPFrameType(value byte) bool {
	switch value {
	case ktpFrameHello, ktpFrameHelloAck, ktpFrameReady, ktpFrameSessionOpen,
		ktpFrameSessionAccept, ktpFrameSessionData, ktpFrameSessionWindow,
		ktpFrameSessionClose, ktpFrameSessionError, ktpFramePing, ktpFramePong,
		ktpFrameStats:
		return true
	default:
		return false
	}
}

func isConnectionLevelKTPFrame(value byte) bool {
	switch value {
	case ktpFrameHello, ktpFrameHelloAck, ktpFrameReady, ktpFramePing, ktpFramePong, ktpFrameStats:
		return true
	default:
		return false
	}
}

func isKnownKTPLeg(value byte) bool {
	return value == ktpLegConnection || value == ktpLegIngress || value == ktpLegEgress
}
```

- [ ] **Step 4: Run Go protocol tests**

Run:

```powershell
go test ./api/client -run TestKTP -count=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add api/client/tunnel_data_protocol.go api/client/tunnel_data_protocol_test.go
git commit -m "Add backend KTP frame codec"
```

---

### Task 6: Backend Tunnel Data State Model

**Files:**
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud\database\models\tunnel.go`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud\database\dbcore\dbcore.go`
- Create: `C:\Users\Administrator\Documents\tanzhen\kelicloud\database\tunnel\data.go`
- Test: `C:\Users\Administrator\Documents\tanzhen\kelicloud\database\tunnel\data_test.go`

- [ ] **Step 1: Write failing data state tests**

Create `database/tunnel/data_test.go`:

```go
package tunnel

import (
	"testing"

	"github.com/komari-monitor/komari/database/models"
	"gorm.io/driver/sqlite"
	"gorm.io/gorm"
)

func newTunnelDataTestDB(t *testing.T) *gorm.DB {
	t.Helper()
	db, err := gorm.Open(sqlite.Open(t.TempDir()+"/tunnel-data.db"), &gorm.Config{})
	if err != nil {
		t.Fatalf("open test db: %v", err)
	}
	if err := db.AutoMigrate(&models.ClientTunnelDataState{}); err != nil {
		t.Fatalf("migrate test db: %v", err)
	}
	return db
}

func TestClientTunnelDataStateMigratesAndStoresReadiness(t *testing.T) {
	db := newTunnelDataTestDB(t)

	err := upsertClientTunnelDataReadyWithDB(db, TunnelDataReady{
		UserID:         "user-a",
		ClientUUID:     "node-a",
		Connected:      true,
		RuleRevision:   "rev-a",
		IngressRuleIDs: []uint{7},
		EgressRuleIDs:  []uint{9},
		LastError:      "",
	})
	if err != nil {
		t.Fatalf("upsert data ready: %v", err)
	}

	var state models.ClientTunnelDataState
	if err := db.Where("user_id = ? AND client_uuid = ?", "user-a", "node-a").First(&state).Error; err != nil {
		t.Fatalf("load data state: %v", err)
	}
	if !state.Connected || state.RuleRevision != "rev-a" || state.IngressRuleIDsJSON != "[7]" || state.EgressRuleIDsJSON != "[9]" {
		t.Fatalf("unexpected data state: %+v", state)
	}
}
```

- [ ] **Step 2: Run failing data state test**

Run:

```powershell
go test ./database/tunnel -run TestClientTunnelDataState -count=1
```

Expected: FAIL with undefined `models.ClientTunnelDataState`.

- [ ] **Step 3: Add data state model and service**

Append to `database/models/tunnel.go`:

```go
type ClientTunnelDataState struct {
	ID                 uint      `json:"id,omitempty" gorm:"primaryKey;autoIncrement"`
	UserID             string    `json:"user_id,omitempty" gorm:"type:varchar(36);not null;index:idx_client_tunnel_data_state_user_client,unique"`
	ClientUUID         string    `json:"client_uuid" gorm:"type:varchar(64);not null;index:idx_client_tunnel_data_state_user_client,unique;index"`
	Connected          bool      `json:"connected" gorm:"not null;default:false;index"`
	RuleRevision       string    `json:"rule_revision" gorm:"type:varchar(128);not null;default:''"`
	IngressRuleIDsJSON string    `json:"ingress_rule_ids_json" gorm:"type:text;not null;default:'[]'"`
	EgressRuleIDsJSON  string    `json:"egress_rule_ids_json" gorm:"type:text;not null;default:'[]'"`
	LastHeartbeatAt    LocalTime `json:"last_heartbeat_at"`
	LastError          string    `json:"last_error" gorm:"type:text"`
	CreatedAt          LocalTime `json:"created_at"`
	UpdatedAt          LocalTime `json:"updated_at"`
}
```

Add to `database/dbcore/dbcore.go` immediately after `&models.ClientTunnelState{}`:

```go
&models.ClientTunnelDataState{},
```

Create `database/tunnel/data.go`:

```go
package tunnel

import (
	"encoding/json"
	"errors"
	"strings"
	"time"

	"github.com/komari-monitor/komari/database/dbcore"
	"github.com/komari-monitor/komari/database/models"
	"gorm.io/gorm"
)

type TunnelDataReady struct {
	UserID         string
	ClientUUID     string
	Connected      bool
	RuleRevision   string
	IngressRuleIDs []uint
	EgressRuleIDs  []uint
	LastError      string
}

func upsertClientTunnelDataReadyWithDB(db *gorm.DB, ready TunnelDataReady) error {
	ingress, err := json.Marshal(ready.IngressRuleIDs)
	if err != nil {
		return err
	}
	egress, err := json.Marshal(ready.EgressRuleIDs)
	if err != nil {
		return err
	}
	now := models.FromTime(time.Now())
	userID := strings.TrimSpace(ready.UserID)
	clientUUID := strings.TrimSpace(ready.ClientUUID)

	var existing models.ClientTunnelDataState
	err = db.Where("user_id = ? AND client_uuid = ?", userID, clientUUID).First(&existing).Error
	if err == nil {
		return db.Model(&models.ClientTunnelDataState{}).Where("id = ?", existing.ID).Updates(map[string]any{
			"connected":             ready.Connected,
			"rule_revision":         strings.TrimSpace(ready.RuleRevision),
			"ingress_rule_ids_json": string(ingress),
			"egress_rule_ids_json":  string(egress),
			"last_heartbeat_at":     now,
			"last_error":            strings.TrimSpace(ready.LastError),
		}).Error
	}
	if !errors.Is(err, gorm.ErrRecordNotFound) {
		return err
	}
	return db.Create(&models.ClientTunnelDataState{
		UserID:             userID,
		ClientUUID:         clientUUID,
		Connected:          ready.Connected,
		RuleRevision:       strings.TrimSpace(ready.RuleRevision),
		IngressRuleIDsJSON: string(ingress),
		EgressRuleIDsJSON:  string(egress),
		LastHeartbeatAt:    now,
		LastError:          strings.TrimSpace(ready.LastError),
	}).Error
}

func UpsertClientTunnelDataReady(ready TunnelDataReady) error {
	return upsertClientTunnelDataReadyWithDB(dbcore.GetDBInstance(), ready)
}

func MarkClientTunnelDataDisconnected(userID, clientUUID, reason string) error {
	return dbcore.GetDBInstance().Model(&models.ClientTunnelDataState{}).
		Where("user_id = ? AND client_uuid = ?", strings.TrimSpace(userID), strings.TrimSpace(clientUUID)).
		Updates(map[string]any{
			"connected":  false,
			"last_error": strings.TrimSpace(reason),
		}).Error
}
```

- [ ] **Step 4: Run data state test**

Run:

```powershell
go test ./database/tunnel -run TestClientTunnelDataState -count=1
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add database/models/tunnel.go database/dbcore/dbcore.go database/tunnel/data.go database/tunnel/data_test.go
git commit -m "Add tunnel data readiness state"
```

---

### Task 7: Backend Data WebSocket Endpoint Skeleton

**Files:**
- Create: `C:\Users\Administrator\Documents\tanzhen\kelicloud\api\client\tunnel_data.go`
- Create: `C:\Users\Administrator\Documents\tanzhen\kelicloud\api\client\tunnel_data_test.go`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud\cmd\server.go`

- [ ] **Step 1: Write failing endpoint helper tests**

Create `api/client/tunnel_data_test.go`:

```go
package client

import (
	"testing"

	"github.com/komari-monitor/komari/database/models"
)

func TestBuildKTPHelloAckFrame(t *testing.T) {
	frame, err := buildKTPHelloAckFrame()
	if err != nil {
		t.Fatalf("build hello ack: %v", err)
	}
	decoded, err := decodeKTPFrame(frame, ktpMaxPayloadLen)
	if err != nil {
		t.Fatalf("decode hello ack: %v", err)
	}
	if decoded.Type != ktpFrameHelloAck || decoded.Leg != ktpLegConnection || decoded.SessionID != 0 {
		t.Fatalf("unexpected hello ack frame: %+v", decoded)
	}
}

func TestParseKTPReadyPayload(t *testing.T) {
	payload := encodeKTPReadyPayloadForTest("rev-a", []uint{7}, []uint{9})

	ready, err := parseKTPReadyPayload(payload)
	if err != nil {
		t.Fatalf("parse ready: %v", err)
	}
	if ready.RuleRevision != "rev-a" || len(ready.IngressRuleIDs) != 1 || len(ready.EgressRuleIDs) != 1 {
		t.Fatalf("unexpected ready payload: %+v", ready)
	}
}

func TestTunnelDataCapabilityAllowsCurrentControlState(t *testing.T) {
	state := models.ClientTunnelState{
		Connected:        true,
		ControlProtocol:  models.TunnelControlProtocolV1,
		CapabilitiesJSON: `["tunnel_control","rule_sync","status_report"]`,
	}

	if !clientTunnelDataAllowedFromControlState(state) {
		t.Fatal("expected current tunnel control state to allow data skeleton")
	}
}
```

- [ ] **Step 2: Run failing endpoint helper tests**

Run:

```powershell
go test ./api/client -run "TestBuildKTPHelloAck|TestParseKTPReady|TestTunnelDataCapability" -count=1
```

Expected: FAIL with undefined helper functions.

- [ ] **Step 3: Implement data endpoint skeleton helpers and handler**

Create `api/client/tunnel_data.go`:

```go
package client

import (
	"encoding/binary"
	"fmt"
	"log"
	"net/http"
	"strings"

	"github.com/gin-gonic/gin"
	"github.com/gorilla/websocket"
	"github.com/komari-monitor/komari/config"
	"github.com/komari-monitor/komari/database/dbcore"
	"github.com/komari-monitor/komari/database/models"
	tunneldb "github.com/komari-monitor/komari/database/tunnel"
)

type ktpReadyPayload struct {
	RuleRevision   string
	IngressRuleIDs []uint
	EgressRuleIDs  []uint
}

func WebSocketTunnelData(c *gin.Context) {
	if !websocket.IsWebSocketUpgrade(c.Request) {
		c.JSON(http.StatusBadRequest, gin.H{"status": "error", "error": "Require WebSocket upgrade"})
		return
	}
	userID, clientUUID, ok := currentTunnelClientScope(c)
	if !ok {
		c.JSON(http.StatusUnauthorized, gin.H{"status": "error", "error": "invalid token"})
		return
	}

	upgrader := websocket.Upgrader{CheckOrigin: func(r *http.Request) bool { return true }}
	allowed, err := config.IsUserFeatureAllowed(userID, config.UserFeatureTunnels)
	if err != nil || !allowed {
		conn, upgradeErr := upgrader.Upgrade(c.Writer, c.Request, nil)
		if upgradeErr == nil {
			_ = conn.WriteMessage(websocket.CloseMessage, websocket.FormatCloseMessage(websocket.ClosePolicyViolation, "feature_disabled"))
			_ = conn.Close()
		}
		return
	}
	if !clientTunnelDataAllowed(userID, clientUUID) {
		conn, upgradeErr := upgrader.Upgrade(c.Writer, c.Request, nil)
		if upgradeErr == nil {
			_ = conn.WriteMessage(websocket.CloseMessage, websocket.FormatCloseMessage(websocket.ClosePolicyViolation, "tunnel_control_required"))
			_ = conn.Close()
		}
		return
	}

	conn, err := upgrader.Upgrade(c.Writer, c.Request, nil)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"status": "error", "error": "Failed to upgrade to WebSocket."+err.Error()})
		return
	}
	defer conn.Close()
	defer func() {
		if err := tunneldb.MarkClientTunnelDataDisconnected(userID, clientUUID, "data socket closed"); err != nil {
			log.Printf("mark tunnel data disconnected %s: %v", clientUUID, err)
		}
	}()

	messageType, payload, err := conn.ReadMessage()
	if err != nil {
		return
	}
	if messageType != websocket.BinaryMessage {
		_ = conn.WriteMessage(websocket.CloseMessage, websocket.FormatCloseMessage(websocket.CloseUnsupportedData, "binary KTP frames required"))
		return
	}
	frame, err := decodeKTPFrame(payload, ktpMaxPayloadLen)
	if err != nil || frame.Type != ktpFrameHello {
		_ = conn.WriteMessage(websocket.CloseMessage, websocket.FormatCloseMessage(websocket.CloseProtocolError, "invalid KTP hello"))
		return
	}
	ack, err := buildKTPHelloAckFrame()
	if err != nil {
		return
	}
	if err := conn.WriteMessage(websocket.BinaryMessage, ack); err != nil {
		return
	}

	for {
		messageType, payload, err := conn.ReadMessage()
		if err != nil {
			return
		}
		if messageType != websocket.BinaryMessage {
			_ = conn.WriteMessage(websocket.CloseMessage, websocket.FormatCloseMessage(websocket.CloseUnsupportedData, "binary KTP frames required"))
			return
		}
		frame, err := decodeKTPFrame(payload, ktpMaxPayloadLen)
		if err != nil {
			_ = conn.WriteMessage(websocket.CloseMessage, websocket.FormatCloseMessage(websocket.CloseProtocolError, "invalid KTP frame"))
			return
		}
		if frame.Type != ktpFrameReady {
			continue
		}
		ready, err := parseKTPReadyPayload(frame.Payload)
		if err != nil {
			_ = conn.WriteMessage(websocket.CloseMessage, websocket.FormatCloseMessage(websocket.CloseProtocolError, "invalid KTP ready"))
			return
		}
		_ = tunneldb.UpsertClientTunnelDataReady(tunneldb.TunnelDataReady{
			UserID:         userID,
			ClientUUID:     clientUUID,
			Connected:      true,
			RuleRevision:   ready.RuleRevision,
			IngressRuleIDs: ready.IngressRuleIDs,
			EgressRuleIDs:  ready.EgressRuleIDs,
			LastError:      "",
		})
	}
}

func clientTunnelDataAllowed(userID, clientUUID string) bool {
	var state models.ClientTunnelState
	err := dbcore.GetDBInstance().
		Where("user_id = ? AND client_uuid = ?", strings.TrimSpace(userID), strings.TrimSpace(clientUUID)).
		First(&state).Error
	if err != nil {
		return false
	}
	return clientTunnelDataAllowedFromControlState(state)
}

func clientTunnelDataAllowedFromControlState(state models.ClientTunnelState) bool {
	return state.Connected &&
		state.ControlProtocol == models.TunnelControlProtocolV1 &&
		strings.Contains(state.CapabilitiesJSON, models.TunnelCapabilityControl)
}

func buildKTPHelloAckFrame() ([]byte, error) {
	payload := make([]byte, 0, 16)
	payload = appendKTPString(payload, "kelicloud-relay")
	payload = append(payload, byte(15))
	frame := ktpFrame{Type: ktpFrameHelloAck, Leg: ktpLegConnection, Payload: payload}
	return encodeKTPFrame(frame)
}

func parseKTPReadyPayload(payload []byte) (ktpReadyPayload, error) {
	revision, rest, err := readKTPString(payload)
	if err != nil {
		return ktpReadyPayload{}, err
	}
	ingress, rest, err := readKTPUintList(rest)
	if err != nil {
		return ktpReadyPayload{}, err
	}
	egress, _, err := readKTPUintList(rest)
	if err != nil {
		return ktpReadyPayload{}, err
	}
	return ktpReadyPayload{RuleRevision: revision, IngressRuleIDs: ingress, EgressRuleIDs: egress}, nil
}

func appendKTPString(out []byte, value string) []byte {
	value = strings.TrimSpace(value)
	out = binary.BigEndian.AppendUint16(out, uint16(len(value)))
	return append(out, []byte(value)...)
}

func readKTPString(payload []byte) (string, []byte, error) {
	if len(payload) < 2 {
		return "", nil, fmt.Errorf("truncated KTP string length")
	}
	length := int(binary.BigEndian.Uint16(payload[:2]))
	payload = payload[2:]
	if len(payload) < length {
		return "", nil, fmt.Errorf("truncated KTP string")
	}
	return string(payload[:length]), payload[length:], nil
}

func readKTPUintList(payload []byte) ([]uint, []byte, error) {
	if len(payload) < 2 {
		return nil, nil, fmt.Errorf("truncated KTP list length")
	}
	count := int(binary.BigEndian.Uint16(payload[:2]))
	payload = payload[2:]
	values := make([]uint, 0, count)
	for index := 0; index < count; index++ {
		if len(payload) < 8 {
			return nil, nil, fmt.Errorf("truncated KTP list value")
		}
		values = append(values, uint(binary.BigEndian.Uint64(payload[:8])))
		payload = payload[8:]
	}
	return values, payload, nil
}

func encodeKTPReadyPayloadForTest(revision string, ingress, egress []uint) []byte {
	out := appendKTPString(nil, revision)
	out = binary.BigEndian.AppendUint16(out, uint16(len(ingress)))
	for _, id := range ingress {
		out = binary.BigEndian.AppendUint64(out, uint64(id))
	}
	out = binary.BigEndian.AppendUint16(out, uint16(len(egress)))
	for _, id := range egress {
		out = binary.BigEndian.AppendUint64(out, uint64(id))
	}
	return out
}
```

- [ ] **Step 4: Register route**

Modify `cmd/server.go`:

```go
tokenAuthrized.GET("/tunnel/data", client.WebSocketTunnelData)
```

Place it immediately after:

```go
tokenAuthrized.GET("/tunnel", client.WebSocketTunnelControl)
```

- [ ] **Step 5: Run endpoint tests**

Run:

```powershell
go test ./api/client -run "TestBuildKTPHelloAck|TestParseKTPReady|TestTunnelDataCapability" -count=1
go test ./cmd/... -count=1
```

Expected: PASS.

- [ ] **Step 6: Commit**

```powershell
git add api/client/tunnel_data.go api/client/tunnel_data_test.go cmd/server.go
git commit -m "Add tunnel data websocket skeleton"
```

---

### Task 8: Verification, Audit, And Publishing

**Files:**
- No feature files should change in this task unless verification finds an issue.

- [ ] **Step 1: Run Rust formatting and targeted tests**

Run in `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs`:

```powershell
cargo fmt
cargo test --test ktp
cargo test --test tunnel_data
cargo test --test protocol --test config --test runtime --test transport
```

Expected: PASS.

- [ ] **Step 2: Run Rust broad smoke tests**

Run:

```powershell
cargo test tunnel
```

Expected: PASS. This should include `tunnel_control`, `tunnel_data`, and tunnel URL/config tests.

- [ ] **Step 3: Run Go targeted tests**

Run in `C:\Users\Administrator\Documents\tanzhen\kelicloud`:

```powershell
go test ./api/client -run "TestKTP|TestBuildKTPHelloAck|TestParseKTPReady|TestTunnelDataCapability|TunnelControl" -count=1
go test ./database/tunnel -run "TestClientTunnelDataState|TestClientTunnelState|TestControlAwareRuleStatus" -count=1
go test ./cmd/... -count=1
```

Expected: PASS. If local Go is unavailable, push backend and verify the GitHub `Build Binaries on Main Push and PR` workflow because it runs `go test ./api/client`, `go test ./database/...`, and `go test ./cmd/...`.

- [ ] **Step 4: Run repository status and diff checks**

Run:

```powershell
git -C C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs status --short
git -C C:\Users\Administrator\Documents\tanzhen\kelicloud status --short
git -C C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs diff --check
git -C C:\Users\Administrator\Documents\tanzhen\kelicloud diff --check
```

Expected:

- both status commands show a clean worktree after commits.
- both diff checks return success.

- [ ] **Step 5: Audit scope invariants**

Run:

```powershell
rg -n "TcpListener|TcpStream::connect|SESSION_DATA|target_host|target_port" C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\src C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\tests
rg -n "net.Listen|Dial|SESSION_DATA|target_host|target_port" C:\Users\Administrator\Documents\tanzhen\kelicloud\api C:\Users\Administrator\Documents\tanzhen\kelicloud\database
```

Expected:

- Rust may show `TcpStream` only inside existing WebSocket transport types, not new ingress listener or target dial code.
- Go may show `target_host` and `target_port` in existing rule/control model, but no new listener, target dial, or byte relay implementation.
- No new code relays `SESSION_DATA` payload bytes.

- [ ] **Step 6: Push agent-rs**

Run:

```powershell
git -C C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs push origin main
```

Expected: push succeeds.

- [ ] **Step 7: Push backend and verify build**

Run:

```powershell
git -C C:\Users\Administrator\Documents\tanzhen\kelicloud push origin main
```

Then query Actions:

```powershell
$sha = git -C C:\Users\Administrator\Documents\tanzhen\kelicloud rev-parse HEAD
$runs = Invoke-RestMethod -Uri "https://api.github.com/repos/keli-123456/kelicloud/actions/runs?head_sha=$sha&per_page=10" -Headers @{ "User-Agent" = "codex"; "Accept" = "application/vnd.github+json" }
$runs.workflow_runs | Select-Object name,status,conclusion,html_url
```

Expected:

- `Build Binaries on Main Push and PR` completes with `success`.
- `Publish Docker Image on Main` starts; if still running, report that honestly with the run URL.

- [ ] **Step 8: Completion audit**

Confirm these statements from current evidence:

- KTP frame format exists in Rust and Go.
- `/api/clients/tunnel/data` is registered.
- Data endpoint authenticates through existing token middleware and feature gate.
- Data endpoint requires current tunnel control capability before data readiness.
- Agent data skeleton is gated by `AGENT_TUNNEL_DATA_ENABLED`.
- Agent data skeleton sends only `HELLO` and `READY`.
- No new code opens ingress listeners.
- No new code dials targets.
- No new code forwards `SESSION_DATA`.
- Existing report, task, ping, and terminal tests still pass.

Only after all statements are proven should the implementation goal be marked complete.

---

## Self-Review

Spec coverage:

- First carrier `KTP over WebSocket Binary over HTTPS`: Tasks 2, 4, 5, and 7.
- KTP frame structure: Tasks 1 and 5.
- ingress, egress, and same-machine loopback modeling: Task 1 preserves `leg`; Task 3 tests skeleton readiness without listener planning; Task 8 audits no runtime forwarding.
- backend relay responsibilities: Task 7 creates data endpoint skeleton and readiness state without relay pairing; actual relay pairing is intentionally outside this skeleton.
- security boundaries: Tasks 6 and 7 enforce token-owned user/client state, `tunnels` feature, and Phase 2 control capability.
- MVP excludes real forwarding: Tasks 3, 4, 7, and 8 explicitly avoid listeners, target dials, and `SESSION_DATA` forwarding.
- testing plan: Tasks 1 through 8 include protocol, agent, backend, and audit tests.

Type consistency:

- Rust frame type names: `FrameType::Hello`, `FrameType::HelloAck`, `FrameType::Ready`, `FrameType::SessionData`.
- Go frame constants: `ktpFrameHello`, `ktpFrameHelloAck`, `ktpFrameReady`, `ktpFrameSessionData`.
- Data URL helper: `build_tunnel_data_ws_url`.
- Data config field: `tunnel_data_enabled`.
- Backend data state model: `ClientTunnelDataState`.
- Backend handler: `WebSocketTunnelData`.

Execution boundary:

- This plan creates skeleton protocol and readiness code only.
- The first real forwarding implementation must be a separate plan after this skeleton is verified.
