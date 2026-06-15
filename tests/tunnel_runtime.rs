use kelicloud_agent_rs::tunnel_control::{SelectedTunnelRule, TunnelRuleStateSink};
use kelicloud_agent_rs::tunnel_data::TunnelDataReadySource;
use kelicloud_agent_rs::tunnel_runtime::{
    build_tcp_listener_plan, source_addr_allowed, SharedTunnelRuleState,
};

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
