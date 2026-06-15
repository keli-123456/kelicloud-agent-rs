use kelicloud_agent_rs::ktp::{FrameLeg, FrameType, KtpFrame};
use kelicloud_agent_rs::tunnel_control::{SelectedTunnelRule, TunnelRuleStateSink};
use kelicloud_agent_rs::tunnel_data::TunnelDataReadySource;
use kelicloud_agent_rs::tunnel_runtime::{
    build_tcp_listener_plan, source_addr_allowed, SharedTunnelRuleState, TunnelSessionRuntime,
    TunnelTcpRuntime,
};
use kelicloud_agent_rs::tunnel_session::encode_session_open_payload;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn tcp_listener_plan_includes_enabled_ingress_and_both_rules_only() {
    let rules = vec![
        selected_rule(7, "tcp", "ingress", true),
        selected_rule(8, "tcp", "both", true),
        selected_rule(9, "tcp", "egress", true),
        selected_rule(10, "udp", "ingress", true),
        selected_rule(11, "tcp", "ingress", false),
    ];

    let plan = build_tcp_listener_plan(&rules);

    assert_eq!(
        plan.iter()
            .map(|listener| listener.rule_id)
            .collect::<Vec<_>>(),
        vec![7, 8]
    );
    assert_eq!(plan[0].listen_address, "127.0.0.1");
    assert_eq!(plan[0].listen_port, 10007);
    assert_eq!(plan[1].source_allowlist, "127.0.0.0/8");
}

#[test]
fn source_allowlist_accepts_empty_wildcard_exact_ip_and_cidr() {
    assert!(source_addr_allowed("203.0.113.9:50000", ""));
    assert!(source_addr_allowed("203.0.113.9:50000", "0.0.0.0/0"));
    assert!(source_addr_allowed("127.0.0.1:50000", "127.0.0.1"));
    assert!(source_addr_allowed("127.4.5.6:50000", "127.0.0.0/8"));
    assert!(!source_addr_allowed("198.51.100.8:50000", "127.0.0.0/8"));
}

#[test]
fn source_allowlist_supports_ipv6_cidr() {
    assert!(source_addr_allowed(
        "[2607:f358:1a::1]:50000",
        "2607:f358:1a::/48"
    ));
    assert!(!source_addr_allowed(
        "[2607:f358:1b::1]:50000",
        "2607:f358:1a::/48"
    ));
}

#[test]
fn shared_tunnel_rule_state_feeds_ready_source_and_listener_plan() {
    let state = SharedTunnelRuleState::new();
    state.update_rules(
        "rev-a",
        &[
            selected_rule(7, "tcp", "ingress", true),
            selected_rule(8, "tcp", "egress", true),
            selected_rule(9, "tcp", "both", true),
        ],
    );

    let ready = state.current_ready();
    assert_eq!(ready.revision, "rev-a");
    assert_eq!(ready.ingress_rule_ids, vec![7, 9]);
    assert_eq!(ready.egress_rule_ids, vec![8, 9]);

    let plan = state.tcp_listener_plan();
    assert_eq!(
        plan.iter()
            .map(|listener| listener.rule_id)
            .collect::<Vec<_>>(),
        vec![7, 9]
    );
}

#[test]
fn tcp_runtime_egress_connects_target_and_queues_response_data() {
    let target = TcpListener::bind("127.0.0.1:0").expect("bind target echo listener");
    let target_addr = target.local_addr().expect("target local addr");
    let echo_thread = thread::spawn(move || {
        let (mut stream, _) = target.accept().expect("accept target connection");
        let mut buffer = [0u8; 16];
        let read = stream.read(&mut buffer).expect("read target input");
        assert_eq!(&buffer[..read], b"ping");
        stream.write_all(b"pong").expect("write target output");
    });

    let state = SharedTunnelRuleState::new();
    let mut rule = selected_rule(7, "tcp", "egress", true);
    rule.target_host = "127.0.0.1".to_string();
    rule.target_port = target_addr.port();
    state.update_rules("rev-a", &[rule]);
    let mut runtime = TunnelTcpRuntime::new(state);
    let open_payload = encode_session_open_payload(
        &kelicloud_agent_rs::tunnel_session::TunnelSessionOpenPayload {
            rule_id: 7,
            listen_host: "127.0.0.1".to_string(),
            listen_port: 10088,
            source_addr: "127.0.0.1:50123".to_string(),
        },
    )
    .expect("encode session open");

    let responses = runtime
        .handle_server_frame(KtpFrame {
            frame_type: FrameType::SessionOpen,
            leg: FrameLeg::Egress,
            flags: 0,
            session_id: 77,
            payload: open_payload,
        })
        .expect("handle session open");
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0].frame_type, FrameType::SessionAccept);
    assert_eq!(responses[0].leg, FrameLeg::Egress);

    runtime
        .handle_server_frame(KtpFrame {
            frame_type: FrameType::SessionData,
            leg: FrameLeg::Egress,
            flags: 0,
            session_id: 77,
            payload: b"ping".to_vec(),
        })
        .expect("handle session data");

    let frame = wait_for_next_runtime_frame(&mut runtime).expect("runtime response frame");
    assert_eq!(frame.frame_type, FrameType::SessionData);
    assert_eq!(frame.leg, FrameLeg::Egress);
    assert_eq!(frame.session_id, 77);
    assert_eq!(frame.payload, b"pong");
    echo_thread.join().expect("echo thread should finish");
}

fn wait_for_next_runtime_frame(runtime: &mut TunnelTcpRuntime) -> Option<KtpFrame> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if let Some(frame) = runtime
            .next_client_frame()
            .expect("poll next runtime frame")
        {
            return Some(frame);
        }
        thread::sleep(Duration::from_millis(10));
    }
    None
}

fn selected_rule(id: u64, protocol: &str, role: &str, enabled: bool) -> SelectedTunnelRule {
    SelectedTunnelRule {
        id,
        name: format!("rule-{id}"),
        enabled,
        protocol: protocol.to_string(),
        role: role.to_string(),
        ingress_group: "edge".to_string(),
        listen_address: "127.0.0.1".to_string(),
        listen_port: 10000 + id as u16,
        egress_group: "rdp".to_string(),
        target_host: "127.0.0.1".to_string(),
        target_port: 3389,
        source_allowlist: "127.0.0.0/8".to_string(),
        max_concurrent_sessions: 32,
        last_revision: 1,
    }
}
