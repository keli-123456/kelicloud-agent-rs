use kelicloud_agent_rs::transport::{HeaderPair, TransportError};
use kelicloud_agent_rs::tunnel_control::{
    build_heartbeat, build_hello, build_rule_ack, parse_server_message, run_tunnel_control_once,
    run_tunnel_control_once_with_rule_sink, run_tunnel_control_session,
    supported_tunnel_data_transports_for_ktp_tcp, RejectedTunnelRule, SelectedTunnelRule,
    TunnelControlClientMessage, TunnelControlServerMessage, TunnelControlSocket,
    TunnelControlTransport, TUNNEL_CONTROL_PROTOCOL_V1,
};
use kelicloud_agent_rs::tunnel_data::SharedTunnelDataReadyState;
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn tunnel_control_hello_declares_capability_without_data_plane() {
    let message = build_hello("0.1.0");
    let json = serde_json::to_string(&message).unwrap();

    assert!(json.contains(r#""type":"hello""#));
    assert!(json.contains(r#""control_protocol":"keli-tunnel-control.v1""#));
    assert!(json.contains(r#""tunnel_control""#));
    assert!(json.contains(r#""rule_sync""#));
    assert!(json.contains(r#""status_report""#));
    assert!(json.contains(r#""data_transports":["websocket","ktp_tcp"]"#));
    assert!(json.contains(r#""data_plane":false"#));
}

#[test]
fn tunnel_control_data_transports_follow_ktp_tcp_configuration() {
    assert_eq!(
        supported_tunnel_data_transports_for_ktp_tcp(false),
        vec!["websocket".to_string()]
    );
    assert_eq!(
        supported_tunnel_data_transports_for_ktp_tcp(true),
        vec!["ktp_tcp".to_string()]
    );
}

#[test]
fn tunnel_control_parses_rule_sync_payload() {
    let message = parse_server_message(
        br#"{
            "type":"rule_sync",
            "revision":"rev-a",
            "rules":[{
                "id":7,
                "name":"RDP",
                "enabled":true,
                "protocol":"tcp",
                "role":"ingress",
                "ingress_group":"edge",
                "listen_address":"0.0.0.0",
                "listen_port":10088,
                "egress_group":"rdp",
                "target_host":"127.0.0.1",
                "target_port":3389,
                "source_allowlist":"0.0.0.0/0",
                "max_concurrent_sessions":32,
                "last_revision":1
            }]
        }"#,
    )
    .unwrap();

    assert_eq!(
        message,
        TunnelControlServerMessage::RuleSync {
            revision: "rev-a".to_string(),
            rules: vec![SelectedTunnelRule {
                id: 7,
                name: "RDP".to_string(),
                enabled: true,
                protocol: "tcp".to_string(),
                role: "ingress".to_string(),
                ingress_group: "edge".to_string(),
                listen_address: "0.0.0.0".to_string(),
                listen_port: 10088,
                egress_group: "rdp".to_string(),
                target_host: "127.0.0.1".to_string(),
                target_port: 3389,
                source_allowlist: "0.0.0.0/0".to_string(),
                max_concurrent_sessions: 32,
                last_revision: 1,
                data_transport: "websocket".to_string(),
            }],
        }
    );
}

#[test]
fn tunnel_control_defaults_missing_rule_data_transport_to_websocket() {
    let message = parse_server_message(
        br#"{
            "type":"rule_sync",
            "revision":"rev-default",
            "rules":[{
                "id":8,
                "name":"SSH",
                "enabled":true,
                "protocol":"tcp",
                "role":"both",
                "ingress_group":"edge",
                "listen_address":"0.0.0.0",
                "listen_port":10022,
                "egress_group":"edge",
                "target_host":"127.0.0.1",
                "target_port":22,
                "source_allowlist":"0.0.0.0/0",
                "max_concurrent_sessions":16,
                "last_revision":2
            }]
        }"#,
    )
    .unwrap();

    let TunnelControlServerMessage::RuleSync { rules, .. } = message else {
        panic!("expected rule sync");
    };

    assert_eq!(rules[0].data_transport, "websocket");
    assert_eq!(rules[0].data_transport(), "websocket");
}

#[test]
fn tunnel_control_parses_ktp_tcp_rule_data_transport() {
    let message = parse_server_message(
        br#"{
            "type":"rule_sync",
            "revision":"rev-ktp",
            "rules":[{
                "id":9,
                "name":"RDP",
                "enabled":true,
                "protocol":"tcp",
                "role":"both",
                "ingress_group":"edge",
                "listen_address":"0.0.0.0",
                "listen_port":10089,
                "egress_group":"edge",
                "target_host":"127.0.0.1",
                "target_port":3389,
                "source_allowlist":"0.0.0.0/0",
                "max_concurrent_sessions":32,
                "last_revision":3,
                "data_transport":"ktp_tcp"
            }]
        }"#,
    )
    .unwrap();

    let TunnelControlServerMessage::RuleSync { rules, .. } = message else {
        panic!("expected rule sync");
    };

    assert_eq!(rules[0].data_transport, "ktp_tcp");
    assert_eq!(rules[0].data_transport(), "ktp_tcp");
}

#[test]
fn tunnel_control_builds_heartbeat_and_rule_ack() {
    let heartbeat = build_heartbeat("rev-a", &[7]);
    assert_eq!(
        heartbeat,
        TunnelControlClientMessage::Heartbeat {
            last_rule_revision: "rev-a".to_string(),
            active_rules: vec![7],
        }
    );

    let accepted = vec![SelectedTunnelRule {
        id: 7,
        name: "RDP".to_string(),
        enabled: true,
        protocol: "tcp".to_string(),
        role: "ingress".to_string(),
        ingress_group: "edge".to_string(),
        listen_address: "0.0.0.0".to_string(),
        listen_port: 10088,
        egress_group: "rdp".to_string(),
        target_host: "127.0.0.1".to_string(),
        target_port: 3389,
        source_allowlist: "0.0.0.0/0".to_string(),
        max_concurrent_sessions: 32,
        last_revision: 1,
        data_transport: "websocket".to_string(),
    }];
    let rejected = vec![RejectedTunnelRule {
        id: 9,
        error: "unsupported protocol".to_string(),
    }];

    assert_eq!(
        build_rule_ack("rev-a", &accepted, &rejected),
        TunnelControlClientMessage::RuleAck {
            revision: "rev-a".to_string(),
            accepted_rule_ids: vec![7],
            rejected_rules: rejected,
        }
    );
}

#[test]
fn tunnel_control_parses_hello_ack() {
    let message = parse_server_message(
        br#"{"type":"hello_ack","server_protocol":"keli-tunnel-control.v1","heartbeat_interval_seconds":15}"#,
    )
    .unwrap();

    assert_eq!(
        message,
        TunnelControlServerMessage::HelloAck {
            server_protocol: TUNNEL_CONTROL_PROTOCOL_V1.to_string(),
            heartbeat_interval_seconds: 15,
        }
    );
}

#[test]
fn tunnel_control_once_acks_rule_sync_and_sends_heartbeat() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut transport = FakeTunnelControlTransport::new(
        events.clone(),
        vec![
            ReadEvent::message(br#"{"type":"hello_ack","server_protocol":"keli-tunnel-control.v1","heartbeat_interval_seconds":15}"#),
            ReadEvent::message(br#"{"type":"rule_sync","revision":"rev-a","rules":[{"id":7,"name":"RDP","enabled":true,"protocol":"tcp","role":"ingress","ingress_group":"edge","listen_address":"0.0.0.0","listen_port":10088,"egress_group":"rdp","target_host":"127.0.0.1","target_port":3389,"source_allowlist":"0.0.0.0/0","max_concurrent_sessions":32,"last_revision":1}]}"#),
        ],
    );

    run_tunnel_control_once(
        "wss://panel.example.com/api/clients/tunnel?token=secret",
        &[],
        "0.1.0",
        &mut transport,
    )
    .unwrap();

    assert_eq!(
        events.borrow()[0],
        "connect:wss://panel.example.com/api/clients/tunnel?token=secret"
    );
    assert!(events
        .borrow()
        .iter()
        .any(|event| event.contains(r#""type":"hello""#)));
    assert!(events
        .borrow()
        .iter()
        .any(|event| event.contains(r#""type":"rule_ack""#)));
    assert!(events
        .borrow()
        .iter()
        .any(|event| event.contains(r#""type":"rule_status""#)));
    assert!(events
        .borrow()
        .iter()
        .any(|event| event.contains(r#""type":"heartbeat""#)));
}

#[test]
fn tunnel_control_rule_sync_updates_shared_ready_state() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut transport = FakeTunnelControlTransport::new(
        events,
        vec![
            ReadEvent::message(br#"{"type":"hello_ack","server_protocol":"keli-tunnel-control.v1","heartbeat_interval_seconds":15}"#),
            ReadEvent::message(br#"{"type":"rule_sync","revision":"rev-a","rules":[{"id":7,"name":"RDP","enabled":true,"protocol":"tcp","role":"both","ingress_group":"edge","listen_address":"0.0.0.0","listen_port":10088,"egress_group":"edge","target_host":"127.0.0.1","target_port":3389,"source_allowlist":"0.0.0.0/0","max_concurrent_sessions":32,"last_revision":1}]}"#),
        ],
    );
    let shared = SharedTunnelDataReadyState::new();

    run_tunnel_control_once_with_rule_sink(
        "wss://panel.example.com/api/clients/tunnel?token=secret",
        &[],
        "0.1.0",
        &mut transport,
        &shared,
    )
    .unwrap();

    let snapshot = shared.snapshot();
    assert_eq!(snapshot.revision, "rev-a");
    assert_eq!(snapshot.ingress_rule_ids, vec![7]);
    assert_eq!(snapshot.egress_rule_ids, vec![7]);
}

#[test]
fn tunnel_control_session_keeps_socket_open_after_idle_read() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut transport = FakeTunnelControlTransport::new(
        events.clone(),
        vec![
            ReadEvent::message(br#"{"type":"hello_ack","server_protocol":"keli-tunnel-control.v1","heartbeat_interval_seconds":15}"#),
            ReadEvent::message(br#"{"type":"rule_sync","revision":"rev-a","rules":[]}"#),
            ReadEvent::idle(),
            ReadEvent::closed(),
        ],
    );

    run_tunnel_control_session(
        "wss://panel.example.com/api/clients/tunnel?token=secret",
        &[],
        "0.1.0",
        &mut transport,
    )
    .unwrap();

    let events = events.borrow();
    let idle_index = events
        .iter()
        .position(|event| event == "read:idle")
        .expect("fake socket should expose an idle read");
    let closed_index = events
        .iter()
        .position(|event| event == "read:closed")
        .expect("session should continue reading until the server closes");
    assert!(
        closed_index > idle_index,
        "persistent control sessions must not close on the first idle read"
    );
}

#[test]
fn tunnel_control_unsupported_endpoint_is_non_fatal() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut transport = FakeTunnelControlTransport::new(events, Vec::new())
        .with_connect_error(TransportError::RequestFailed("HTTP 404".to_string()));

    let result = run_tunnel_control_once(
        "wss://panel.example.com/api/clients/tunnel?token=secret",
        &[],
        "0.1.0",
        &mut transport,
    );

    assert!(result.is_ok());
}

#[test]
fn tungstenite_tunnel_control_connector_redacts_token_in_startup_line() {
    let line = kelicloud_agent_rs::tunnel_control::tunnel_control_startup_line(
        "wss://panel.example.com/api/clients/tunnel?token=secret",
        true,
    );

    assert_eq!(
        line,
        "tunnel control: enabled url=wss://panel.example.com/api/clients/tunnel?token=redacted"
    );
}

struct FakeTunnelControlTransport {
    events: Rc<RefCell<Vec<String>>>,
    inbound: Vec<ReadEvent>,
    connect_error: Option<TransportError>,
}

impl FakeTunnelControlTransport {
    fn new(events: Rc<RefCell<Vec<String>>>, inbound: Vec<ReadEvent>) -> Self {
        Self {
            events,
            inbound,
            connect_error: None,
        }
    }

    fn with_connect_error(mut self, error: TransportError) -> Self {
        self.connect_error = Some(error);
        self
    }
}

impl TunnelControlTransport for FakeTunnelControlTransport {
    type Socket = FakeTunnelControlSocket;

    fn connect_tunnel_control(
        &mut self,
        url: &str,
        _headers: &[HeaderPair],
    ) -> Result<Self::Socket, TransportError> {
        self.events.borrow_mut().push(format!("connect:{url}"));
        if let Some(error) = self.connect_error.take() {
            return Err(error);
        }
        Ok(FakeTunnelControlSocket {
            events: self.events.clone(),
            inbound: self.inbound.drain(..).collect(),
        })
    }
}

struct FakeTunnelControlSocket {
    events: Rc<RefCell<Vec<String>>>,
    inbound: Vec<ReadEvent>,
}

impl TunnelControlSocket for FakeTunnelControlSocket {
    fn send_message(&mut self, message: &TunnelControlClientMessage) -> Result<(), TransportError> {
        self.events
            .borrow_mut()
            .push(serde_json::to_string(message).unwrap());
        Ok(())
    }

    fn read_message(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        if self.inbound.is_empty() {
            self.events.borrow_mut().push("read:idle".to_string());
            return Ok(None);
        }
        match self.inbound.remove(0) {
            ReadEvent::Message(bytes) => {
                self.events.borrow_mut().push("read:message".to_string());
                Ok(Some(bytes))
            }
            ReadEvent::Idle => {
                self.events.borrow_mut().push("read:idle".to_string());
                Ok(None)
            }
            ReadEvent::Closed => {
                self.events.borrow_mut().push("read:closed".to_string());
                Err(TransportError::SocketClosed)
            }
        }
    }
}

enum ReadEvent {
    Message(Vec<u8>),
    Idle,
    Closed,
}

impl ReadEvent {
    fn message(bytes: &[u8]) -> Self {
        Self::Message(bytes.to_vec())
    }

    fn idle() -> Self {
        Self::Idle
    }

    fn closed() -> Self {
        Self::Closed
    }
}
