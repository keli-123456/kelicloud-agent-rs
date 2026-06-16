use kelicloud_agent_rs::tunnel_preflight::{
    check_listener_bindable, validate_tunnel_tcp_rule, TunnelPreflightIssueCode,
    TunnelTcpRulePreflightInput,
};

#[test]
fn preflight_accepts_bindable_loopback_rule_on_linux() {
    let port = free_tcp_port();
    let input = TunnelTcpRulePreflightInput {
        rule_id: 7,
        listen_address: "127.0.0.1".to_string(),
        listen_port: port,
        target_host: "127.0.0.1".to_string(),
        target_port: 3389,
        source_allowlist: "127.0.0.0/8".to_string(),
    };

    let issues = validate_tunnel_tcp_rule(&input);

    if cfg!(target_os = "linux") {
        assert_eq!(issues, Vec::new());
    } else {
        let codes = issues
            .into_iter()
            .map(|issue| issue.code)
            .collect::<Vec<_>>();
        assert_eq!(codes, vec![TunnelPreflightIssueCode::UnsupportedOs]);
    }
}

#[test]
fn preflight_reports_invalid_target_and_allowlist() {
    let input = TunnelTcpRulePreflightInput {
        rule_id: 8,
        listen_address: "127.0.0.1".to_string(),
        listen_port: 10088,
        target_host: "".to_string(),
        target_port: 0,
        source_allowlist: "bad-cidr/999".to_string(),
    };

    let codes = validate_tunnel_tcp_rule(&input)
        .into_iter()
        .map(|issue| issue.code)
        .collect::<Vec<_>>();

    assert!(codes.contains(&TunnelPreflightIssueCode::InvalidTarget));
    assert!(codes.contains(&TunnelPreflightIssueCode::InvalidAllowlist));
}

#[test]
fn preflight_reports_listener_bind_failure() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind occupied listener");
    let port = listener.local_addr().expect("local addr").port();

    let issue = check_listener_bindable("127.0.0.1", port).expect("occupied port should fail");

    assert_eq!(issue.code, TunnelPreflightIssueCode::ListenBindFailed);
    assert!(issue.message.contains("127.0.0.1"));
}

fn free_tcp_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind free port");
    listener.local_addr().expect("local addr").port()
}
