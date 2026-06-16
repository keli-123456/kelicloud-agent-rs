use std::cell::RefCell;
use std::rc::Rc;

use kelicloud_agent_rs::ktp::{
    decode_frame, encode_frame, FrameLeg, FrameType, KtpFrame, KTP_MAX_PAYLOAD_LEN,
};
use kelicloud_agent_rs::transport::{HeaderPair, TransportError};
use kelicloud_agent_rs::tunnel_control::SelectedTunnelRule;
use kelicloud_agent_rs::tunnel_data::{
    run_tunnel_data_once, run_tunnel_data_session, run_tunnel_data_session_with_ready_source,
    run_tunnel_data_session_with_ready_source_and_runtime, tunnel_data_startup_line,
    SharedTunnelDataReadyState, TungsteniteTunnelDataTransport, TunnelDataReadyState,
    TunnelDataRuleFailure, TunnelDataSocket, TunnelDataTransport,
};
use kelicloud_agent_rs::tunnel_runtime::TunnelSessionRuntime;

type OptionalReadHook = Rc<RefCell<dyn FnMut(usize)>>;

struct FakeTunnelDataTransport {
    events: Rc<RefCell<Vec<String>>>,
    inbound: Vec<Result<Vec<u8>, TransportError>>,
    connect_error: Option<TransportError>,
    send_error_after: usize,
    send_error: Option<TransportError>,
    optional_read_hook: Option<OptionalReadHook>,
}

struct FakeTunnelDataSocket {
    events: Rc<RefCell<Vec<String>>>,
    inbound: Vec<Result<Vec<u8>, TransportError>>,
    send_error_after: usize,
    send_error: Option<TransportError>,
    send_count: usize,
    optional_read_hook: Option<OptionalReadHook>,
    optional_read_count: usize,
}

impl FakeTunnelDataTransport {
    fn with_hello_ack(events: Rc<RefCell<Vec<String>>>) -> Self {
        Self {
            events,
            inbound: vec![Ok(hello_ack_frame())],
            connect_error: None,
            send_error_after: usize::MAX,
            send_error: None,
            optional_read_hook: None,
        }
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
            events: Rc::clone(&self.events),
            inbound: self.inbound.drain(..).collect(),
            send_error_after: self.send_error_after,
            send_error: self.send_error.take(),
            send_count: 0,
            optional_read_hook: self.optional_read_hook.take(),
            optional_read_count: 0,
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

    fn read_frame(&mut self) -> Result<Vec<u8>, TransportError> {
        self.events.borrow_mut().push("read".to_string());
        self.inbound.remove(0).map_err(|error| error.clone())
    }

    fn read_optional_frame(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        self.events.borrow_mut().push("read_optional".to_string());
        self.optional_read_count += 1;
        if let Some(hook) = &self.optional_read_hook {
            (hook.borrow_mut())(self.optional_read_count);
        }
        self.inbound
            .remove(0)
            .map(Some)
            .map_err(|error| error.clone())
    }
}

#[test]
fn tunnel_data_once_sends_hello_and_ready_without_listener_plan() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut transport = FakeTunnelDataTransport::with_hello_ack(Rc::clone(&events));
    let mut ready = TunnelDataReadyState::empty("rev-a");
    ready.ingress_rule_ids.push(7);
    ready.egress_rule_ids.push(9);
    ready.failed_rules.push(TunnelDataRuleFailure {
        rule_id: 7,
        status: "listen_bind_failed".to_string(),
        error: "cannot bind listener 127.0.0.1:10088".to_string(),
    });

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
    assert_eq!(events.len(), 4);
    assert_eq!(
        events[0],
        "connect:wss://panel.example.com/api/clients/tunnel/data?token=secret"
    );
    assert!(events[1].starts_with("frame:"));
    assert_eq!(events[2], "read");
    assert!(events[3].starts_with("frame:"));

    let hello = decode_frame(
        &hex_to_bytes(events[1].strip_prefix("frame:").expect("frame prefix")),
        KTP_MAX_PAYLOAD_LEN,
    )
    .expect("hello frame should decode");
    let ready = decode_frame(
        &hex_to_bytes(events[3].strip_prefix("frame:").expect("frame prefix")),
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
    assert_eq!(
        ready_payload.failed_rules,
        [(
            7,
            "listen_bind_failed".to_string(),
            "cannot bind listener 127.0.0.1:10088".to_string(),
        )]
    );
}

#[test]
fn tunnel_data_ready_state_derives_rule_ids_from_selected_roles() {
    let rules = vec![
        selected_rule(7, "ingress"),
        selected_rule(8, "egress"),
        selected_rule(9, "both"),
    ];

    let ready = TunnelDataReadyState::from_selected_rules("rev-a", &rules);

    assert_eq!(ready.revision, "rev-a");
    assert_eq!(ready.ingress_rule_ids, vec![7, 9]);
    assert_eq!(ready.egress_rule_ids, vec![8, 9]);
    assert!(ready.failed_rules.is_empty());
}

fn selected_rule(id: u64, role: &str) -> SelectedTunnelRule {
    SelectedTunnelRule {
        id,
        name: format!("rule-{id}"),
        enabled: true,
        protocol: "tcp".to_string(),
        role: role.to_string(),
        ingress_group: "edge".to_string(),
        listen_address: "0.0.0.0".to_string(),
        listen_port: 10000 + id as u16,
        egress_group: "rdp".to_string(),
        target_host: "127.0.0.1".to_string(),
        target_port: 3389,
        source_allowlist: "0.0.0.0/0".to_string(),
        max_concurrent_sessions: 32,
        last_revision: 1,
    }
}

#[test]
fn tunnel_data_requires_hello_ack_before_ready() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut transport = FakeTunnelDataTransport {
        events: Rc::clone(&events),
        inbound: vec![Ok(encode_frame(&KtpFrame::connection(
            FrameType::Ping,
            Vec::new(),
        ))
        .expect("ping frame should encode"))],
        connect_error: None,
        send_error_after: usize::MAX,
        send_error: None,
        optional_read_hook: None,
    };

    let error = run_tunnel_data_once(
        "wss://panel.example.com/api/clients/tunnel/data?token=secret",
        &[],
        "node-a",
        "0.1.0",
        &TunnelDataReadyState::empty("rev-1"),
        &mut transport,
    )
    .expect_err("data tunnel must reject non-hello_ack responses");

    match error {
        TransportError::RequestFailed(message) => {
            assert!(message.contains("hello_ack"), "unexpected error: {message}");
        }
        other => panic!("expected request failed, got {other:?}"),
    }

    let sent_frame_count = events
        .borrow()
        .iter()
        .filter(|event| event.starts_with("frame:"))
        .count();
    assert_eq!(sent_frame_count, 1, "READY must wait for HELLO_ACK");
}

#[test]
fn tunnel_data_session_keeps_socket_open_after_ready() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut transport = FakeTunnelDataTransport {
        events: Rc::clone(&events),
        inbound: vec![Ok(hello_ack_frame()), Err(TransportError::SocketClosed)],
        connect_error: None,
        send_error_after: usize::MAX,
        send_error: None,
        optional_read_hook: None,
    };

    run_tunnel_data_session(
        "wss://panel.example.com/api/clients/tunnel/data?token=secret",
        &[],
        "node-a",
        "0.1.0",
        &TunnelDataReadyState::empty("rev-1"),
        &mut transport,
    )
    .expect("data session should treat server close as a clean reconnect boundary");

    let events = events.borrow();
    let ready_index = events
        .iter()
        .position(|event| {
            event.starts_with("frame:") && frame_type_from_event(event) == FrameType::Ready
        })
        .expect("session should send READY");
    let post_ready_read_index = events
        .iter()
        .position(|event| event == "read_optional")
        .expect("session should keep reading after READY");
    assert!(
        post_ready_read_index > ready_index,
        "persistent data sessions must not close immediately after READY"
    );
}

#[test]
fn tunnel_data_session_resends_ready_when_shared_state_changes() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let shared = SharedTunnelDataReadyState::new();
    let update_shared = shared.clone();
    let hook: OptionalReadHook = Rc::new(RefCell::new(move |count| {
        if count == 1 {
            update_shared.update_from_selected_rules("rev-a", &[selected_rule(7, "both")]);
        }
    }));
    let mut transport = FakeTunnelDataTransport {
        events: Rc::clone(&events),
        inbound: vec![
            Ok(hello_ack_frame()),
            Ok(encode_frame(&KtpFrame::connection(FrameType::Ping, Vec::new())).unwrap()),
            Err(TransportError::SocketClosed),
        ],
        connect_error: None,
        send_error_after: usize::MAX,
        send_error: None,
        optional_read_hook: Some(hook),
    };

    run_tunnel_data_session_with_ready_source(
        "wss://panel.example.com/api/clients/tunnel/data?token=secret",
        &[],
        "node-a",
        "0.1.0",
        &shared,
        &mut transport,
    )
    .expect("data session should finish at reconnect boundary");

    let ready_payloads = events
        .borrow()
        .iter()
        .filter(|event| event.starts_with("frame:"))
        .filter_map(|event| {
            let frame = decode_frame(
                &hex_to_bytes(event.strip_prefix("frame:").expect("frame prefix")),
                KTP_MAX_PAYLOAD_LEN,
            )
            .expect("frame should decode");
            (frame.frame_type == FrameType::Ready).then(|| parse_ready_payload(&frame.payload))
        })
        .collect::<Vec<_>>();

    assert_eq!(ready_payloads.len(), 2);
    assert!(ready_payloads[0].ingress_rule_ids.is_empty());
    assert_eq!(ready_payloads[1].revision, "rev-a");
    assert_eq!(ready_payloads[1].ingress_rule_ids, vec![7]);
    assert_eq!(ready_payloads[1].egress_rule_ids, vec![7]);
}

#[test]
fn tunnel_data_session_dispatches_session_frames_to_runtime_and_sends_responses() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let server_frame = encode_frame(&KtpFrame {
        frame_type: FrameType::SessionAccept,
        leg: FrameLeg::Ingress,
        flags: 0,
        session_id: 77,
        payload: 7u64.to_be_bytes().to_vec(),
    })
    .expect("session accept frame should encode");
    let mut transport = FakeTunnelDataTransport {
        events: Rc::clone(&events),
        inbound: vec![
            Ok(hello_ack_frame()),
            Ok(server_frame),
            Err(TransportError::SocketClosed),
        ],
        connect_error: None,
        send_error_after: usize::MAX,
        send_error: None,
        optional_read_hook: None,
    };
    let mut runtime = FakeSessionRuntime {
        handled: Vec::new(),
        response: Some(KtpFrame {
            frame_type: FrameType::SessionData,
            leg: FrameLeg::Ingress,
            flags: 0,
            session_id: 77,
            payload: b"ok".to_vec(),
        }),
    };

    run_tunnel_data_session_with_ready_source_and_runtime(
        "wss://panel.example.com/api/clients/tunnel/data?token=secret",
        &[],
        "node-a",
        "0.1.0",
        &TunnelDataReadyState::empty("rev-1"),
        &mut transport,
        &mut runtime,
    )
    .expect("data session should dispatch server session frame");

    assert_eq!(runtime.handled.len(), 1);
    assert_eq!(runtime.handled[0].frame_type, FrameType::SessionAccept);
    assert_eq!(runtime.handled[0].session_id, 77);

    let sent_session_frames = sent_frames(&events)
        .into_iter()
        .filter(|frame| frame.frame_type == FrameType::SessionData)
        .collect::<Vec<_>>();
    assert_eq!(sent_session_frames.len(), 1);
    assert_eq!(sent_session_frames[0].payload, b"ok");
}

#[test]
fn main_wires_tunnel_control_state_into_data_ready_source() {
    let source = std::fs::read_to_string("src/main.rs").expect("main source should be readable");

    assert!(source.contains("SharedTunnelRuleState::new()"));
    assert!(source.contains("TunnelTcpRuntime::new"));
    assert!(source.contains("run_tunnel_control_once_with_rule_sink"));
    assert!(source.contains("run_tunnel_control_session_with_rule_sink"));
    assert!(source.contains("run_tunnel_data_session_with_ready_source_and_runtime"));
}

#[test]
fn tunnel_data_unsupported_endpoint_is_non_fatal() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut transport = FakeTunnelDataTransport {
        events,
        inbound: vec![Ok(hello_ack_frame())],
        connect_error: Some(TransportError::RequestFailed("status=404".to_string())),
        send_error_after: usize::MAX,
        send_error: None,
        optional_read_hook: None,
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
            inbound: vec![Ok(hello_ack_frame())],
            connect_error: None,
            send_error_after,
            send_error: Some(TransportError::SocketClosed),
            optional_read_hook: None,
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
            inbound: vec![Ok(hello_ack_frame())],
            connect_error: None,
            send_error_after: usize::MAX,
            send_error: None,
            optional_read_hook: None,
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
            inbound: vec![Ok(hello_ack_frame())],
            connect_error: None,
            send_error_after: usize::MAX,
            send_error: None,
            optional_read_hook: None,
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
        inbound: vec![Ok(hello_ack_frame())],
        connect_error: None,
        send_error_after: 0,
        send_error: Some(TransportError::SocketClosed),
        optional_read_hook: None,
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

struct FakeSessionRuntime {
    handled: Vec<KtpFrame>,
    response: Option<KtpFrame>,
}

impl TunnelSessionRuntime for FakeSessionRuntime {
    fn handle_server_frame(&mut self, frame: KtpFrame) -> Result<Vec<KtpFrame>, TransportError> {
        self.handled.push(frame);
        Ok(self.response.take().into_iter().collect())
    }
}

fn hello_ack_frame() -> Vec<u8> {
    encode_frame(&KtpFrame::connection(FrameType::HelloAck, Vec::new()))
        .expect("hello_ack frame should encode")
}

fn frame_type_from_event(event: &str) -> FrameType {
    decode_frame(
        &hex_to_bytes(event.strip_prefix("frame:").expect("frame prefix")),
        KTP_MAX_PAYLOAD_LEN,
    )
    .expect("frame should decode")
    .frame_type
}

fn sent_frames(events: &Rc<RefCell<Vec<String>>>) -> Vec<KtpFrame> {
    events
        .borrow()
        .iter()
        .filter(|event| event.starts_with("frame:"))
        .map(|event| {
            decode_frame(
                &hex_to_bytes(event.strip_prefix("frame:").expect("frame prefix")),
                KTP_MAX_PAYLOAD_LEN,
            )
            .expect("frame should decode")
        })
        .collect()
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

#[test]
fn tunnel_data_startup_line_reports_disabled() {
    assert_eq!(tunnel_data_startup_line("", false), "tunnel data: disabled");
}

#[test]
fn tungstenite_tunnel_data_transport_can_be_constructed() {
    let _transport = TungsteniteTunnelDataTransport::new_with_custom_dns("8.8.8.8");
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
