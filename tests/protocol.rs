use kelicloud_agent_rs::protocol::{
    build_report_ws_url, build_terminal_ws_url, parse_backend_message, BackendMessage,
};

#[test]
fn report_ws_url_converts_https_endpoint_to_wss() {
    let url = build_report_ws_url("https://panel.example.com/", "token-123").unwrap();

    assert_eq!(
        url,
        "wss://panel.example.com/api/clients/report?token=token-123"
    );
}

#[test]
fn report_ws_url_converts_http_endpoint_to_ws_and_escapes_token() {
    let url = build_report_ws_url("http://127.0.0.1:25774", "token with/slash").unwrap();

    assert_eq!(
        url,
        "ws://127.0.0.1:25774/api/clients/report?token=token%20with%2Fslash"
    );
}

#[test]
fn report_ws_url_converts_idn_host_to_ascii_like_go_agent() {
    let url = build_report_ws_url("https://例子.测试:8443/base/", "tok").unwrap();

    assert_eq!(
        url,
        "wss://xn--fsqu00a.xn--0zwm56d:8443/base/api/clients/report?token=tok"
    );
}

#[test]
fn terminal_ws_url_adds_token_and_terminal_id() {
    let url = build_terminal_ws_url("https://panel.example.com/base/", "tok", "term-1").unwrap();

    assert_eq!(
        url,
        "wss://panel.example.com/base/api/clients/terminal?token=tok&id=term-1"
    );
}

#[test]
fn terminal_ws_url_converts_idn_host_to_ascii_like_go_agent() {
    let url = build_terminal_ws_url("https://中文域名.com/base/", "tok", "term-1").unwrap();

    assert_eq!(
        url,
        "wss://xn--fiq06l2rdsvs.com/base/api/clients/terminal?token=tok&id=term-1"
    );
}

#[test]
fn report_ws_url_rejects_empty_endpoint() {
    let err = build_report_ws_url("  ", "token-123").unwrap_err();

    assert!(err.to_string().contains("endpoint"));
}

#[test]
fn report_ws_url_rejects_empty_token() {
    let err = build_report_ws_url("https://panel.example.com", " ").unwrap_err();

    assert!(err.to_string().contains("token"));
}

#[test]
fn parses_cn_connectivity_probe_config_message() {
    let message = parse_backend_message(
        br#"{
            "message":"cn_connectivity_probe_config",
            "cn_connectivity_enabled":true,
            "cn_connectivity_target":"223.5.5.5",
            "cn_connectivity_interval":60,
            "cn_connectivity_retry_attempts":3,
            "cn_connectivity_retry_delay_seconds":1,
            "cn_connectivity_timeout_seconds":5
        }"#,
    )
    .unwrap();

    assert_eq!(
        message,
        BackendMessage::CnConnectivityProbeConfig {
            enabled: true,
            target: Some("223.5.5.5".to_string()),
            interval_seconds: 60,
            retry_attempts: 3,
            retry_delay_seconds: 1,
            timeout_seconds: 5,
        }
    );
}

#[test]
fn parses_terminal_message_from_request_id() {
    let message =
        parse_backend_message(br#"{"message":"terminal","request_id":"term-1"}"#).unwrap();

    assert_eq!(
        message,
        BackendMessage::Terminal {
            request_id: "term-1".to_string()
        }
    );
}

#[test]
fn parses_exec_message() {
    let message =
        parse_backend_message(br#"{"message":"exec","task_id":"task-1","command":"whoami"}"#)
            .unwrap();

    assert_eq!(
        message,
        BackendMessage::Exec {
            task_id: "task-1".to_string(),
            command: "whoami".to_string(),
        }
    );
}

#[test]
fn parses_ping_message() {
    let message = parse_backend_message(
        br#"{"message":"ping","ping_task_id":42,"ping_type":"tcp","ping_target":"1.1.1.1:443"}"#,
    )
    .unwrap();

    assert_eq!(
        message,
        BackendMessage::Ping {
            task_id: 42,
            ping_type: "tcp".to_string(),
            target: "1.1.1.1:443".to_string(),
        }
    );
}

#[test]
fn preserves_unknown_backend_message_name() {
    let message = parse_backend_message(br#"{"message":"new_feature","value":1}"#).unwrap();

    assert_eq!(
        message,
        BackendMessage::Unknown {
            message: Some("new_feature".to_string())
        }
    );
}
