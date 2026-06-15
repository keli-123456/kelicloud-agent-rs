use kelicloud_agent_rs::tunnel_control::{
    build_heartbeat, build_hello, build_rule_ack, parse_server_message, RejectedTunnelRule,
    SelectedTunnelRule, TunnelControlClientMessage, TunnelControlServerMessage,
    TUNNEL_CONTROL_PROTOCOL_V1,
};

#[test]
fn tunnel_control_hello_declares_capability_without_data_plane() {
    let message = build_hello("0.1.0");
    let json = serde_json::to_string(&message).unwrap();

    assert!(json.contains(r#""type":"hello""#));
    assert!(json.contains(r#""control_protocol":"keli-tunnel-control.v1""#));
    assert!(json.contains(r#""tunnel_control""#));
    assert!(json.contains(r#""rule_sync""#));
    assert!(json.contains(r#""status_report""#));
    assert!(json.contains(r#""data_plane":false"#));
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
            }],
        }
    );
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
