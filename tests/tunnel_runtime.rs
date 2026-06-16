use kelicloud_agent_rs::ktp::{FrameLeg, FrameType, KtpFrame};
use kelicloud_agent_rs::tunnel_control::{SelectedTunnelRule, TunnelRuleStateSink};
use kelicloud_agent_rs::tunnel_data::TunnelDataReadySource;
use kelicloud_agent_rs::tunnel_runtime::{
    build_tcp_listener_plan, source_addr_allowed, SharedTunnelRuleState, TunnelSessionRuntime,
    TunnelTcpRuntime,
};
use kelicloud_agent_rs::tunnel_session::{
    decode_session_error_payload, encode_session_open_payload,
};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
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
    let mut ingress_rule = selected_rule(7, "tcp", "ingress", true);
    ingress_rule.listen_port = free_tcp_port();
    let egress_rule = selected_rule(8, "tcp", "egress", true);
    let mut both_rule = selected_rule(9, "tcp", "both", true);
    both_rule.listen_port = free_tcp_port();
    state.update_rules("rev-a", &[ingress_rule, egress_rule, both_rule]);

    let ready = state.current_ready();
    assert_eq!(ready.revision, "rev-a");
    if cfg!(target_os = "linux") {
        assert!(ready.ingress_rule_ids.is_empty());
        assert_eq!(ready.egress_rule_ids, vec![8, 9]);
        for rule_id in [7, 9] {
            assert!(
                ready.failed_rules.iter().any(|failure| {
                    failure.rule_id == rule_id && failure.status == "listener_stopped"
                }),
                "expected listener_stopped failure for {rule_id}, got {:?}",
                ready.failed_rules
            );
        }
    } else {
        assert!(ready.ingress_rule_ids.is_empty());
        assert!(ready.egress_rule_ids.is_empty());
        for rule_id in [7, 8, 9] {
            assert!(
                ready.failed_rules.iter().any(|failure| {
                    failure.rule_id == rule_id && failure.status == "unsupported_os"
                }),
                "expected unsupported_os failure for {rule_id}, got {:?}",
                ready.failed_rules
            );
        }
    }

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

#[test]
fn tcp_runtime_ingress_listener_queues_open_data_and_writes_server_response() {
    let listen_port = free_tcp_port();
    let state = SharedTunnelRuleState::new();
    let mut rule = selected_rule(17, "tcp", "ingress", true);
    rule.listen_address = "127.0.0.1".to_string();
    rule.listen_port = listen_port;
    rule.source_allowlist = "127.0.0.0/8".to_string();
    state.update_rules("rev-a", &[rule]);
    let mut runtime = TunnelTcpRuntime::new(state);
    runtime.refresh_listeners().expect("start ingress listener");

    let client_thread = thread::spawn(move || {
        let mut stream = connect_with_retry(("127.0.0.1", listen_port));
        stream.write_all(b"hello").expect("write ingress input");
        let mut buffer = [0u8; 16];
        let read = stream.read(&mut buffer).expect("read ingress response");
        assert_eq!(&buffer[..read], b"world");
    });

    let open = wait_for_next_runtime_frame(&mut runtime).expect("session open frame");
    assert_eq!(open.frame_type, FrameType::SessionOpen);
    assert_eq!(open.leg, FrameLeg::Ingress);
    assert_ne!(open.session_id, 0);

    let data = wait_for_next_runtime_frame(&mut runtime).expect("session data frame");
    assert_eq!(data.frame_type, FrameType::SessionData);
    assert_eq!(data.session_id, open.session_id);
    assert_eq!(data.payload, b"hello");

    runtime
        .handle_server_frame(KtpFrame {
            frame_type: FrameType::SessionData,
            leg: FrameLeg::Ingress,
            flags: 0,
            session_id: open.session_id,
            payload: b"world".to_vec(),
        })
        .expect("write server response to ingress client");

    client_thread.join().expect("client thread should finish");
}

#[test]
fn tcp_runtime_stops_listener_when_rule_is_removed() {
    let listen_port = free_tcp_port();
    let state = SharedTunnelRuleState::new();
    let mut rule = selected_rule(41, "tcp", "ingress", true);
    rule.listen_address = "127.0.0.1".to_string();
    rule.listen_port = listen_port;
    state.update_rules("rev-a", &[rule]);
    let mut runtime = TunnelTcpRuntime::new(state.clone());
    runtime.refresh_listeners().expect("start listener");
    assert!(TcpStream::connect(("127.0.0.1", listen_port)).is_ok());

    state.update_rules("rev-b", &[]);
    runtime.refresh_listeners().expect("stop removed listener");

    assert_port_eventually_closed(listen_port);
}

#[test]
fn tcp_runtime_restarts_listener_when_listen_port_changes() {
    let first_port = free_tcp_port();
    let second_port = free_tcp_port();
    let state = SharedTunnelRuleState::new();
    let mut rule = selected_rule(42, "tcp", "ingress", true);
    rule.listen_address = "127.0.0.1".to_string();
    rule.listen_port = first_port;
    state.update_rules("rev-a", &[rule.clone()]);
    let mut runtime = TunnelTcpRuntime::new(state.clone());
    runtime.refresh_listeners().expect("start first listener");
    assert!(TcpStream::connect(("127.0.0.1", first_port)).is_ok());

    rule.listen_port = second_port;
    state.update_rules("rev-b", &[rule]);
    runtime.refresh_listeners().expect("restart listener");

    assert_port_eventually_closed(first_port);
    connect_with_retry(("127.0.0.1", second_port));
}

#[test]
fn tcp_runtime_removes_session_after_local_close() {
    let listen_port = free_tcp_port();
    let state = SharedTunnelRuleState::new();
    let mut rule = selected_rule(43, "tcp", "ingress", true);
    rule.listen_address = "127.0.0.1".to_string();
    rule.listen_port = listen_port;
    state.update_rules("rev-a", &[rule]);
    let mut runtime = TunnelTcpRuntime::new(state);
    runtime.refresh_listeners().expect("start ingress listener");

    let stream = connect_with_retry(("127.0.0.1", listen_port));
    let open = wait_for_next_runtime_frame(&mut runtime).expect("session open frame");
    assert_ne!(open.session_id, 0);
    assert_session_count_eventually(&runtime, 1);

    drop(stream);
    let close = wait_for_next_runtime_frame(&mut runtime).expect("session close frame");
    assert_eq!(close.frame_type, FrameType::SessionClose);
    assert_eq!(close.session_id, open.session_id);
    assert_session_count_eventually(&runtime, 0);
}

#[test]
fn tcp_runtime_two_agent_relay_simulation_forwards_echo() {
    let target = TcpListener::bind("127.0.0.1:0").expect("bind target echo listener");
    let target_addr = target.local_addr().expect("target local addr");
    let echo_thread = thread::spawn(move || {
        let (mut stream, _) = target.accept().expect("accept target connection");
        let mut buffer = [0u8; 16];
        let read = stream.read(&mut buffer).expect("read target input");
        assert_eq!(&buffer[..read], b"ping");
        stream.write_all(b"pong").expect("write target output");
    });

    let listen_port = free_tcp_port();
    let ingress_state = SharedTunnelRuleState::new();
    let mut ingress_rule = selected_rule(31, "tcp", "ingress", true);
    ingress_rule.listen_address = "127.0.0.1".to_string();
    ingress_rule.listen_port = listen_port;
    ingress_rule.source_allowlist = "127.0.0.0/8".to_string();
    ingress_state.update_rules("rev-a", &[ingress_rule]);
    let mut ingress_runtime = TunnelTcpRuntime::new(ingress_state);
    ingress_runtime
        .refresh_listeners()
        .expect("start ingress listener");

    let egress_state = SharedTunnelRuleState::new();
    let mut egress_rule = selected_rule(31, "tcp", "egress", true);
    egress_rule.target_host = "127.0.0.1".to_string();
    egress_rule.target_port = target_addr.port();
    egress_state.update_rules("rev-a", &[egress_rule]);
    let mut egress_runtime = TunnelTcpRuntime::new(egress_state);

    let client_thread = thread::spawn(move || {
        let mut stream = connect_with_retry(("127.0.0.1", listen_port));
        stream.write_all(b"ping").expect("write ingress input");
        let mut buffer = [0u8; 16];
        let read = stream.read(&mut buffer).expect("read ingress response");
        assert_eq!(&buffer[..read], b"pong");
    });

    let open = wait_for_next_runtime_frame(&mut ingress_runtime).expect("ingress open frame");
    assert_eq!(open.frame_type, FrameType::SessionOpen);
    let mut open_to_egress = open.clone();
    open_to_egress.leg = FrameLeg::Egress;
    let egress_responses = egress_runtime
        .handle_server_frame(open_to_egress)
        .expect("egress handles session open");
    for mut frame in egress_responses {
        frame.leg = FrameLeg::Ingress;
        ingress_runtime
            .handle_server_frame(frame)
            .expect("ingress handles egress response");
    }

    let data = wait_for_next_runtime_frame(&mut ingress_runtime).expect("ingress data frame");
    assert_eq!(data.frame_type, FrameType::SessionData);
    assert_eq!(data.payload, b"ping");
    let mut data_to_egress = data.clone();
    data_to_egress.leg = FrameLeg::Egress;
    egress_runtime
        .handle_server_frame(data_to_egress)
        .expect("egress handles ingress data");

    let mut target_data =
        wait_for_next_runtime_frame(&mut egress_runtime).expect("egress target data frame");
    assert_eq!(target_data.frame_type, FrameType::SessionData);
    assert_eq!(target_data.payload, b"pong");
    target_data.leg = FrameLeg::Ingress;
    ingress_runtime
        .handle_server_frame(target_data)
        .expect("ingress writes target response");

    client_thread.join().expect("client thread should finish");
    echo_thread.join().expect("echo thread should finish");
}

#[test]
fn tcp_runtime_target_connect_failure_returns_stable_error_code_and_no_session() {
    let closed_port = free_tcp_port();
    let state = SharedTunnelRuleState::new();
    let mut rule = selected_rule(51, "tcp", "egress", true);
    rule.target_host = "127.0.0.1".to_string();
    rule.target_port = closed_port;
    state.update_rules("rev-a", &[rule]);
    let mut runtime = TunnelTcpRuntime::new(state);

    let payload = encode_session_open_payload(
        &kelicloud_agent_rs::tunnel_session::TunnelSessionOpenPayload {
            rule_id: 51,
            listen_host: "127.0.0.1".to_string(),
            listen_port: 10088,
            source_addr: "127.0.0.1:50123".to_string(),
        },
    )
    .expect("encode open payload");

    let responses = runtime
        .handle_server_frame(KtpFrame {
            frame_type: FrameType::SessionOpen,
            leg: FrameLeg::Egress,
            flags: 0,
            session_id: 510,
            payload,
        })
        .expect("handle open failure");

    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0].frame_type, FrameType::SessionError);
    let error = decode_session_error_payload(&responses[0].payload).expect("decode session error");
    assert_eq!(error.rule_id, 51);
    assert_eq!(error.code, "target_connect_failed");
    assert_eq!(runtime.active_session_count(), 0);
}

#[test]
fn tcp_runtime_missing_egress_rule_returns_runtime_unavailable() {
    let state = SharedTunnelRuleState::new();
    let mut runtime = TunnelTcpRuntime::new(state);
    let payload = encode_session_open_payload(
        &kelicloud_agent_rs::tunnel_session::TunnelSessionOpenPayload {
            rule_id: 88,
            listen_host: "127.0.0.1".to_string(),
            listen_port: 10088,
            source_addr: "127.0.0.1:50123".to_string(),
        },
    )
    .expect("encode open payload");

    let responses = runtime
        .handle_server_frame(KtpFrame {
            frame_type: FrameType::SessionOpen,
            leg: FrameLeg::Egress,
            flags: 0,
            session_id: 880,
            payload,
        })
        .expect("handle missing rule");

    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0].frame_type, FrameType::SessionError);
    let error = decode_session_error_payload(&responses[0].payload).expect("decode session error");
    assert_eq!(error.rule_id, 88);
    assert_eq!(error.code, "runtime_unavailable");
    assert_eq!(runtime.active_session_count(), 0);
}

#[test]
fn shared_tunnel_rule_state_reports_invalid_target_as_egress_failure() {
    let state = SharedTunnelRuleState::new();
    let mut rule = selected_rule(61, "tcp", "egress", true);
    rule.target_host = "".to_string();
    rule.target_port = 0;
    state.update_rules("rev-preflight", &[rule]);

    let ready = state.current_ready();

    assert_eq!(ready.revision, "rev-preflight");
    assert!(ready.egress_rule_ids.is_empty());
    assert!(
        ready
            .failed_rules
            .iter()
            .any(|failure| { failure.rule_id == 61 && failure.status == "invalid_target" }),
        "expected invalid_target failure, got {:?}",
        ready.failed_rules
    );
}

#[test]
fn shared_tunnel_rule_state_reports_invalid_allowlist_as_ingress_failure() {
    let state = SharedTunnelRuleState::new();
    let mut rule = selected_rule(62, "tcp", "ingress", true);
    rule.source_allowlist = "bad-cidr/999".to_string();
    state.update_rules("rev-preflight", &[rule]);

    let ready = state.current_ready();

    assert!(ready.ingress_rule_ids.is_empty());
    assert!(
        ready
            .failed_rules
            .iter()
            .any(|failure| { failure.rule_id == 62 && failure.status == "invalid_allowlist" }),
        "expected invalid_allowlist failure, got {:?}",
        ready.failed_rules
    );
}

#[test]
fn shared_tunnel_rule_state_combines_preflight_and_listener_health_failures() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let state = SharedTunnelRuleState::new();
    let mut rule = selected_rule(68, "tcp", "ingress", true);
    rule.listen_address = "127.0.0.1".to_string();
    rule.listen_port = free_tcp_port();
    rule.source_allowlist = "bad-cidr/999".to_string();
    state.update_rules("rev-combined-failure", &[rule.clone()]);
    let spec = build_tcp_listener_plan(&[rule]).remove(0);
    state.set_listener_runtime_error(spec, "accept failed: socket closed");

    let ready = state.current_ready();

    assert!(!ready.ingress_rule_ids.contains(&68));
    assert!(
        ready
            .failed_rules
            .iter()
            .any(|failure| failure.rule_id == 68 && failure.status == "invalid_allowlist"),
        "expected invalid_allowlist failure, got {:?}",
        ready.failed_rules
    );
    assert!(
        ready.failed_rules.iter().any(|failure| {
            failure.rule_id == 68
                && failure.status == "listener_runtime_error"
                && failure.error.contains("accept failed")
        }),
        "expected listener_runtime_error failure, got {:?}",
        ready.failed_rules
    );
}

#[test]
fn shared_tunnel_rule_state_blocks_ingress_when_listener_bind_fails() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind occupied listener");
    let port = listener.local_addr().expect("listener addr").port();
    let state = SharedTunnelRuleState::new();
    let mut rule = selected_rule(63, "tcp", "both", true);
    rule.listen_address = "127.0.0.1".to_string();
    rule.listen_port = port;
    state.update_rules("rev-preflight", &[rule]);

    let ready = state.current_ready();

    assert!(!ready.ingress_rule_ids.contains(&63));
    assert!(
        ready
            .failed_rules
            .iter()
            .any(|failure| { failure.rule_id == 63 && failure.status == "listen_bind_failed" }),
        "expected listen_bind_failed failure, got {:?}",
        ready.failed_rules
    );
    if cfg!(target_os = "linux") {
        assert!(ready.egress_rule_ids.contains(&63));
    } else {
        assert!(
            ready
                .failed_rules
                .iter()
                .any(|failure| { failure.rule_id == 63 && failure.status == "unsupported_os" }),
            "expected unsupported_os failure on non-Linux, got {:?}",
            ready.failed_rules
        );
    }
}

#[test]
fn shared_tunnel_rule_state_keeps_runtime_owned_listener_ready_after_refresh() {
    let listen_port = free_tcp_port();
    let state = SharedTunnelRuleState::new();
    let mut rule = selected_rule(64, "tcp", "ingress", true);
    rule.listen_address = "127.0.0.1".to_string();
    rule.listen_port = listen_port;
    rule.source_allowlist = "127.0.0.0/8".to_string();
    state.update_rules("rev-runtime-listener", &[rule]);
    let mut runtime = TunnelTcpRuntime::new(state.clone());
    runtime.refresh_listeners().expect("start ingress listener");

    let ready = state.current_ready();

    assert!(
        !ready
            .failed_rules
            .iter()
            .any(|failure| { failure.rule_id == 64 && failure.status == "listen_bind_failed" }),
        "runtime-owned listener must not be reported as bind failure: {:?}",
        ready.failed_rules
    );
    if cfg!(target_os = "linux") {
        assert!(ready.ingress_rule_ids.contains(&64));
    } else {
        assert!(
            ready
                .failed_rules
                .iter()
                .any(|failure| { failure.rule_id == 64 && failure.status == "unsupported_os" }),
            "unsupported_os must still be reported on non-Linux: {:?}",
            ready.failed_rules
        );
    }
}

#[test]
fn tcp_runtime_next_client_frame_does_not_refresh_listeners() {
    let listen_port = free_tcp_port();
    let state = SharedTunnelRuleState::new();
    let mut rule = selected_rule(73, "tcp", "ingress", true);
    rule.listen_address = "127.0.0.1".to_string();
    rule.listen_port = listen_port;
    state.update_rules("rev-next-frame-no-refresh", &[rule]);
    let mut runtime = TunnelTcpRuntime::new(state);

    assert!(runtime
        .next_client_frame()
        .expect("polling client frames should not fail")
        .is_none());
    assert!(
        TcpStream::connect(("127.0.0.1", listen_port)).is_err(),
        "next_client_frame must not start listeners"
    );

    runtime.tick().expect("tick should refresh listeners");
    connect_with_retry(("127.0.0.1", listen_port));
}

#[test]
fn shared_tunnel_rule_state_blocks_ingress_when_listener_health_is_missing() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let state = SharedTunnelRuleState::new();
    let mut rule = selected_rule(65, "tcp", "ingress", true);
    rule.listen_address = "127.0.0.1".to_string();
    rule.listen_port = free_tcp_port();
    state.update_rules("rev-runtime-health", &[rule]);

    let ready = state.current_ready();

    assert!(!ready.ingress_rule_ids.contains(&65));
    assert!(
        ready
            .failed_rules
            .iter()
            .any(|failure| failure.rule_id == 65 && failure.status == "listener_stopped"),
        "expected listener_stopped failure, got {:?}",
        ready.failed_rules
    );
}

#[test]
fn shared_tunnel_rule_state_requires_active_listener_for_running_health() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let state = SharedTunnelRuleState::new();
    let mut rule = selected_rule(72, "tcp", "ingress", true);
    rule.listen_address = "127.0.0.1".to_string();
    rule.listen_port = free_tcp_port();
    state.update_rules("rev-running-without-active", &[rule.clone()]);
    let spec = build_tcp_listener_plan(&[rule]).remove(0);
    state.set_listener_running(spec);

    let ready = state.current_ready();

    assert!(!ready.ingress_rule_ids.contains(&72));
    assert!(
        ready
            .failed_rules
            .iter()
            .any(|failure| failure.rule_id == 72 && failure.status == "listener_stopped"),
        "running health without active listener must report listener_stopped, got {:?}",
        ready.failed_rules
    );
}

#[test]
fn shared_tunnel_rule_state_reports_runtime_listener_error() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let state = SharedTunnelRuleState::new();
    let mut rule = selected_rule(66, "tcp", "ingress", true);
    rule.listen_address = "127.0.0.1".to_string();
    rule.listen_port = free_tcp_port();
    rule.source_allowlist = "127.0.0.0/8".to_string();
    state.update_rules("rev-runtime-error", &[rule.clone()]);
    let spec = build_tcp_listener_plan(&[rule]).remove(0);
    let mut runtime = TunnelTcpRuntime::new(state.clone());
    runtime.refresh_listeners().expect("start ingress listener");
    state.set_listener_running(spec.clone());

    let running = state.current_ready();
    assert!(
        running.ingress_rule_ids.contains(&66),
        "expected listener-running rule to be ready, got {:?}",
        running
    );

    state.set_listener_runtime_error(spec, "accept failed: socket closed");
    let failed = state.current_ready();

    assert!(!failed.ingress_rule_ids.contains(&66));
    assert!(
        failed.failed_rules.iter().any(|failure| {
            failure.rule_id == 66
                && failure.status == "listener_runtime_error"
                && failure.error.contains("accept failed")
        }),
        "expected listener_runtime_error failure, got {:?}",
        failed.failed_rules
    );
}

#[test]
fn shared_tunnel_rule_state_reports_listener_start_failed_health() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let state = SharedTunnelRuleState::new();
    let mut rule = selected_rule(69, "tcp", "ingress", true);
    rule.listen_address = "127.0.0.1".to_string();
    rule.listen_port = free_tcp_port();
    state.update_rules("rev-start-failed", &[rule.clone()]);
    let spec = build_tcp_listener_plan(&[rule]).remove(0);
    state.set_listener_start_failed(spec, "bind failed: address in use");

    let ready = state.current_ready();

    assert!(!ready.ingress_rule_ids.contains(&69));
    assert!(
        ready.failed_rules.iter().any(|failure| {
            failure.rule_id == 69
                && failure.status == "listener_start_failed"
                && failure.error.contains("address in use")
        }),
        "expected listener_start_failed failure, got {:?}",
        ready.failed_rules
    );
}

#[test]
fn tcp_runtime_start_failure_reports_listener_start_failed() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let occupied = TcpListener::bind("127.0.0.1:0").expect("bind occupied listener");
    let port = occupied
        .local_addr()
        .expect("occupied listener addr")
        .port();
    let state = SharedTunnelRuleState::new();
    let mut rule = selected_rule(68, "tcp", "ingress", true);
    rule.listen_address = "127.0.0.1".to_string();
    rule.listen_port = port;
    state.update_rules("rev-start-failed", &[rule]);
    let mut runtime = TunnelTcpRuntime::new(state.clone());

    runtime
        .refresh_listeners()
        .expect("listener start failure is reported through READY");
    let ready = state.current_ready();

    assert!(!ready.ingress_rule_ids.contains(&68));
    assert!(
        ready
            .failed_rules
            .iter()
            .any(|failure| failure.rule_id == 68 && failure.status == "listener_start_failed"),
        "expected listener_start_failed failure, got {:?}",
        ready.failed_rules
    );
}

#[test]
fn tcp_runtime_recovers_listener_health_after_start_failure_is_cleared() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let occupied = TcpListener::bind("127.0.0.1:0").expect("bind occupied listener");
    let port = occupied
        .local_addr()
        .expect("occupied listener addr")
        .port();
    let state = SharedTunnelRuleState::new();
    let mut rule = selected_rule(69, "tcp", "ingress", true);
    rule.listen_address = "127.0.0.1".to_string();
    rule.listen_port = port;
    state.update_rules("rev-recovery", &[rule]);
    let mut runtime = TunnelTcpRuntime::new(state.clone());

    runtime
        .refresh_listeners()
        .expect("listener start failure is reported through READY");
    let failed = state.current_ready();
    assert!(
        failed
            .failed_rules
            .iter()
            .any(|failure| failure.rule_id == 69 && failure.status == "listener_start_failed"),
        "expected listener_start_failed before recovery, got {:?}",
        failed.failed_rules
    );

    drop(occupied);
    runtime
        .refresh_listeners()
        .expect("listener should recover after the port is released");
    let recovered = state.current_ready();

    assert!(
        recovered.ingress_rule_ids.contains(&69),
        "expected recovered listener to become ready, got {:?}",
        recovered
    );
    assert!(
        recovered
            .failed_rules
            .iter()
            .all(|failure| failure.rule_id != 69 || failure.status != "listener_start_failed"),
        "start failure must clear after recovery, got {:?}",
        recovered.failed_rules
    );
}

#[test]
fn shared_tunnel_rule_state_reports_explicit_listener_stopped_health() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let state = SharedTunnelRuleState::new();
    let mut rule = selected_rule(70, "tcp", "ingress", true);
    rule.listen_address = "127.0.0.1".to_string();
    rule.listen_port = free_tcp_port();
    state.update_rules("rev-stopped", &[rule.clone()]);
    let spec = build_tcp_listener_plan(&[rule]).remove(0);
    state.set_listener_stopped(spec, "listener stopped unexpectedly");

    let ready = state.current_ready();

    assert!(!ready.ingress_rule_ids.contains(&70));
    assert!(
        ready.failed_rules.iter().any(|failure| {
            failure.rule_id == 70
                && failure.status == "listener_stopped"
                && failure.error.contains("stopped unexpectedly")
        }),
        "expected listener_stopped failure, got {:?}",
        ready.failed_rules
    );
}

#[test]
fn shared_tunnel_rule_state_skips_listener_health_when_os_is_unsupported() {
    if cfg!(target_os = "linux") {
        return;
    }
    let state = SharedTunnelRuleState::new();
    let mut rule = selected_rule(71, "tcp", "ingress", true);
    rule.listen_address = "127.0.0.1".to_string();
    rule.listen_port = free_tcp_port();
    state.update_rules("rev-unsupported-listener-health", &[rule.clone()]);
    let spec = build_tcp_listener_plan(&[rule]).remove(0);
    state.set_listener_runtime_error(spec, "accept failed: socket closed");

    let ready = state.current_ready();

    assert!(
        ready
            .failed_rules
            .iter()
            .any(|failure| failure.rule_id == 71 && failure.status == "unsupported_os"),
        "expected unsupported_os failure, got {:?}",
        ready.failed_rules
    );
    assert!(
        ready
            .failed_rules
            .iter()
            .all(|failure| !failure.status.starts_with("listener_")),
        "listener health must be skipped when unsupported_os is present: {:?}",
        ready.failed_rules
    );
}

#[test]
fn shared_tunnel_rule_state_clears_stale_listener_health_when_rule_changes() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let state = SharedTunnelRuleState::new();
    let mut old_rule = selected_rule(67, "tcp", "ingress", true);
    old_rule.listen_address = "127.0.0.1".to_string();
    old_rule.listen_port = free_tcp_port();
    let old_listen_port = old_rule.listen_port;
    state.update_rules("rev-old", &[old_rule.clone()]);
    let old_spec = build_tcp_listener_plan(&[old_rule]).remove(0);
    state.set_listener_runtime_error(old_spec, "old listener failed");

    let mut new_rule = selected_rule(67, "tcp", "ingress", true);
    new_rule.listen_address = "127.0.0.1".to_string();
    new_rule.listen_port = old_listen_port;
    new_rule.source_allowlist = "127.0.0.1".to_string();
    state.update_rules("rev-new", &[new_rule]);

    let ready = state.current_ready();

    assert!(
        ready
            .failed_rules
            .iter()
            .all(|failure| !failure.error.contains("old listener failed")),
        "stale listener health must be removed after rule change: {:?}",
        ready.failed_rules
    );
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

fn free_tcp_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind free port");
    listener.local_addr().expect("local addr").port()
}

fn connect_with_retry(addr: (&str, u16)) -> TcpStream {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match TcpStream::connect(addr) {
            Ok(stream) => return stream,
            Err(error) if Instant::now() < deadline => {
                let _ = error;
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("connect ingress listener: {error}"),
        }
    }
}

fn assert_port_eventually_closed(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_err() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("port {port} remained open");
}

fn assert_session_count_eventually(runtime: &TunnelTcpRuntime, expected: usize) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if runtime.active_session_count() == expected {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(runtime.active_session_count(), expected);
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
