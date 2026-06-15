use std::cell::RefCell;
use std::rc::Rc;

use kelicloud_agent_rs::ktp::{decode_frame, FrameType, KTP_MAX_PAYLOAD_LEN};
use kelicloud_agent_rs::transport::{HeaderPair, TransportError};
use kelicloud_agent_rs::tunnel_data::{
    run_tunnel_data_once, tunnel_data_startup_line, TunnelDataReadyState, TunnelDataRuleFailure,
    TunnelDataSocket, TunnelDataTransport,
};

struct FakeTunnelDataTransport {
    events: Rc<RefCell<Vec<String>>>,
    connect_error: Option<TransportError>,
    send_error_after: usize,
    send_error: Option<TransportError>,
}

struct FakeTunnelDataSocket {
    events: Rc<RefCell<Vec<String>>>,
    send_error_after: usize,
    send_error: Option<TransportError>,
    send_count: usize,
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
            events: Rc::clone(&self.events),
            send_error_after: self.send_error_after,
            send_error: self.send_error.take(),
            send_count: 0,
        })
    }
}

impl TunnelDataSocket for FakeTunnelDataSocket {
    fn send_frame(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        self.events
            .borrow_mut()
            .push(format!("frame:{}", bytes_to_hex(frame)));
        if self.send_count == self.send_error_after {
            if let Some(error) = self.send_error.take() {
                return Err(error);
            }
        }
        self.send_count += 1;
        Ok(())
    }
}

#[test]
fn tunnel_data_once_sends_hello_and_ready_without_listener_plan() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut transport = FakeTunnelDataTransport {
        events: Rc::clone(&events),
        connect_error: None,
        send_error_after: usize::MAX,
        send_error: None,
    };
    let mut ready = TunnelDataReadyState::empty("rev-a");
    ready.ingress_rule_ids.push(7);
    ready.egress_rule_ids.push(9);

    run_tunnel_data_once(
        "wss://panel.example.com/api/clients/tunnel/data?token=secret",
        &[],
        "node-a",
        "0.1.0",
        &ready,
        &mut transport,
    )
    .expect("tunnel data once should succeed");

    let events = events.borrow();
    assert_eq!(events.len(), 3);
    assert_eq!(
        events[0],
        "connect:wss://panel.example.com/api/clients/tunnel/data?token=secret"
    );
    assert!(events[1].starts_with("frame:"));
    assert!(events[2].starts_with("frame:"));

    let hello = decode_frame(
        &hex_to_bytes(events[1].strip_prefix("frame:").expect("frame prefix")),
        KTP_MAX_PAYLOAD_LEN,
    )
    .expect("hello frame should decode");
    let ready = decode_frame(
        &hex_to_bytes(events[2].strip_prefix("frame:").expect("frame prefix")),
        KTP_MAX_PAYLOAD_LEN,
    )
    .expect("ready frame should decode");

    assert_eq!(hello.frame_type, FrameType::Hello);
    assert_eq!(ready.frame_type, FrameType::Ready);

    let hello_payload = parse_hello_payload(&hello.payload);
    assert_eq!(hello_payload.agent_id_hint, "node-a");
    assert_eq!(hello_payload.agent_version, "0.1.0");
    assert_eq!(hello_payload.revision, "rev-a");
    assert_eq!(
        hello_payload.capabilities,
        ["tcp", "multiplex", "flow_control", "stats"]
    );

    let ready_payload = parse_ready_payload(&ready.payload);
    assert_eq!(ready_payload.revision, "rev-a");
    assert_eq!(ready_payload.ingress_rule_ids, [7]);
    assert_eq!(ready_payload.egress_rule_ids, [9]);
    assert!(ready_payload.failed_rules.is_empty());
}

#[test]
fn tunnel_data_unsupported_endpoint_is_non_fatal() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut transport = FakeTunnelDataTransport {
        events,
        connect_error: Some(TransportError::RequestFailed("status=404".to_string())),
        send_error_after: usize::MAX,
        send_error: None,
    };
    let ready = TunnelDataReadyState::empty("rev-1");

    run_tunnel_data_once(
        "wss://panel.example.com/api/clients/tunnel/data?token=secret",
        &[],
        "node-a",
        "0.1.0",
        &ready,
        &mut transport,
    )
    .expect("unsupported tunnel data endpoint should be non-fatal");
}

#[test]
fn tunnel_data_send_socket_closed_is_non_fatal() {
    for send_error_after in [0, 1] {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut transport = FakeTunnelDataTransport {
            events,
            connect_error: None,
            send_error_after,
            send_error: Some(TransportError::SocketClosed),
        };
        let ready = TunnelDataReadyState::empty("rev-1");

        run_tunnel_data_once(
            "wss://panel.example.com/api/clients/tunnel/data?token=secret",
            &[],
            "node-a",
            "0.1.0",
            &ready,
            &mut transport,
        )
        .expect("socket closed during tunnel data send should be non-fatal");
    }
}

#[test]
fn tunnel_data_rejects_oversized_payload_fields_without_truncating() {
    let oversized = "x".repeat(u16::MAX as usize + 1);

    let agent_id_error = run_tunnel_data_once(
        "wss://panel.example.com/api/clients/tunnel/data?token=secret",
        &[],
        &oversized,
        "0.1.0",
        &TunnelDataReadyState::empty("rev-1"),
        &mut FakeTunnelDataTransport {
            events: Rc::new(RefCell::new(Vec::new())),
            connect_error: None,
            send_error_after: usize::MAX,
            send_error: None,
        },
    )
    .expect_err("oversized agent id should be rejected");
    assert_request_failed_too_long(agent_id_error);

    let mut ready = TunnelDataReadyState::empty("rev-1");
    ready.failed_rules.push(TunnelDataRuleFailure {
        rule_id: 7,
        status: oversized,
        error: "boom".to_string(),
    });
    let status_error = run_tunnel_data_once(
        "wss://panel.example.com/api/clients/tunnel/data?token=secret",
        &[],
        "node-a",
        "0.1.0",
        &ready,
        &mut FakeTunnelDataTransport {
            events: Rc::new(RefCell::new(Vec::new())),
            connect_error: None,
            send_error_after: usize::MAX,
            send_error: None,
        },
    )
    .expect_err("oversized failed rule status should be rejected");
    assert_request_failed_too_long(status_error);
}

#[test]
fn tunnel_data_send_socket_closed_stops_without_ready() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut transport = FakeTunnelDataTransport {
        events: Rc::clone(&events),
        connect_error: None,
        send_error_after: 0,
        send_error: Some(TransportError::SocketClosed),
    };

    run_tunnel_data_once(
        "wss://panel.example.com/api/clients/tunnel/data?token=secret",
        &[],
        "node-a",
        "0.1.0",
        &TunnelDataReadyState::empty("rev-1"),
        &mut transport,
    )
    .expect("socket closed during hello send should be non-fatal");

    let events = events.borrow();
    let sent_frame_count = events
        .iter()
        .filter(|event| event.starts_with("frame:"))
        .count();
    assert_eq!(sent_frame_count, 1);
    let hello = decode_frame(
        &hex_to_bytes(events[1].strip_prefix("frame:").expect("frame prefix")),
        KTP_MAX_PAYLOAD_LEN,
    )
    .expect("hello frame should decode");
    assert_eq!(hello.frame_type, FrameType::Hello);
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

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    assert_eq!(hex.len() % 2, 0);
    hex.as_bytes()
        .chunks(2)
        .map(|chunk| {
            let text = std::str::from_utf8(chunk).expect("hex should be valid utf-8");
            u8::from_str_radix(text, 16).expect("hex byte should parse")
        })
        .collect()
}

struct HelloPayload {
    agent_id_hint: String,
    agent_version: String,
    revision: String,
    capabilities: Vec<String>,
}

struct ReadyPayload {
    revision: String,
    ingress_rule_ids: Vec<u64>,
    egress_rule_ids: Vec<u64>,
    failed_rules: Vec<(u64, String, String)>,
}

fn parse_hello_payload(payload: &[u8]) -> HelloPayload {
    let mut cursor = PayloadCursor::new(payload);
    let hello = HelloPayload {
        agent_id_hint: cursor.read_string(),
        agent_version: cursor.read_string(),
        revision: cursor.read_string(),
        capabilities: cursor.read_string_list(),
    };
    cursor.expect_end();
    hello
}

fn parse_ready_payload(payload: &[u8]) -> ReadyPayload {
    let mut cursor = PayloadCursor::new(payload);
    let revision = cursor.read_string();
    let ingress_rule_ids = cursor.read_u64_list();
    let egress_rule_ids = cursor.read_u64_list();
    let failed_count = cursor.read_u16();
    let mut failed_rules = Vec::new();
    for _ in 0..failed_count {
        failed_rules.push((
            cursor.read_u64(),
            cursor.read_string(),
            cursor.read_string(),
        ));
    }
    cursor.expect_end();
    ReadyPayload {
        revision,
        ingress_rule_ids,
        egress_rule_ids,
        failed_rules,
    }
}

fn assert_request_failed_too_long(error: TransportError) {
    match error {
        TransportError::RequestFailed(message) => {
            let message = message.to_ascii_lowercase();
            assert!(
                message.contains("too long") || message.contains("exceeds u16"),
                "unexpected error message: {message}"
            );
        }
        other => panic!("expected request failed error, got {other:?}"),
    }
}

struct PayloadCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> PayloadCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn read_string(&mut self) -> String {
        let len = self.read_u16() as usize;
        let end = self.offset + len;
        let value = std::str::from_utf8(&self.bytes[self.offset..end])
            .expect("payload string should be utf-8")
            .to_string();
        self.offset = end;
        value
    }

    fn read_string_list(&mut self) -> Vec<String> {
        let count = self.read_u16();
        (0..count).map(|_| self.read_string()).collect()
    }

    fn read_u64_list(&mut self) -> Vec<u64> {
        let count = self.read_u16();
        (0..count).map(|_| self.read_u64()).collect()
    }

    fn read_u16(&mut self) -> u16 {
        let end = self.offset + 2;
        let value = u16::from_be_bytes(
            self.bytes[self.offset..end]
                .try_into()
                .expect("payload u16 should fit"),
        );
        self.offset = end;
        value
    }

    fn read_u64(&mut self) -> u64 {
        let end = self.offset + 8;
        let value = u64::from_be_bytes(
            self.bytes[self.offset..end]
                .try_into()
                .expect("payload u64 should fit"),
        );
        self.offset = end;
        value
    }

    fn expect_end(&self) {
        assert_eq!(self.offset, self.bytes.len());
    }
}
