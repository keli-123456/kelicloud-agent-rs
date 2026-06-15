use kelicloud_agent_rs::config::{AgentConfig, ConfigError};
use std::fs;

fn env_lookup(key: &str) -> Option<String> {
    match key {
        "AGENT_ENDPOINT" => Some("https://env.example.com".to_string()),
        "AGENT_TOKEN" => Some("env-token".to_string()),
        "AGENT_INSECURE" => Some("true".to_string()),
        _ => None,
    }
}

#[test]
fn config_reads_endpoint_and_token_from_environment() {
    let args = ["kelicloud-agent-rs"];
    let config = AgentConfig::from_args_and_env(args, env_lookup).unwrap();

    assert_eq!(config.endpoint, "https://env.example.com");
    assert_eq!(config.token, "env-token");
    assert!(config.insecure);
    assert_eq!(config.interval_seconds, 1.0);
    assert_eq!(config.max_retries, 3);
    assert_eq!(config.reconnect_interval_seconds, 5);
    assert_eq!(config.info_report_interval_minutes, 5);
    assert_eq!(config.cf_access_client_id, "");
    assert_eq!(config.cf_access_client_secret, "");
    assert!(config.tunnel_control_enabled);
    assert!(!config.once);
}

#[test]
fn config_can_disable_tunnel_control_from_environment() {
    let config = AgentConfig::from_args_and_env(["kelicloud-agent-rs"], |key| match key {
        "AGENT_ENDPOINT" => Some("https://env.example.com".to_string()),
        "AGENT_TOKEN" => Some("env-token".to_string()),
        "AGENT_TUNNEL_CONTROL_ENABLED" => Some("disabled".to_string()),
        _ => None,
    })
    .unwrap();

    assert!(!config.tunnel_control_enabled);
}

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

#[test]
fn config_environment_overrides_command_line_like_go_agent() {
    let args = [
        "kelicloud-agent-rs",
        "--endpoint",
        "https://cli.example.com",
        "--token",
        "cli-token",
    ];
    let config = AgentConfig::from_args_and_env(args, env_lookup).unwrap();

    assert_eq!(config.endpoint, "https://env.example.com");
    assert_eq!(config.token, "env-token");
    assert!(config.insecure);
}

#[test]
fn config_environment_overrides_command_line_metric_options_like_go_agent() {
    let args = [
        "kelicloud-agent-rs",
        "--endpoint",
        "https://cli.example.com",
        "--token",
        "cli-token",
        "--include-nics",
        "cli0",
        "--exclude-nics",
        "cli1",
        "--include-mountpoint",
        "/cli",
        "--custom-ipv4",
        "203.0.113.10",
        "--custom-ipv6",
        "2001:db8::10",
        "--custom-dns",
        "1.1.1.1",
        "--get-ip-addr-from-nic",
        "--memory-include-cache",
        "--memory-exclude-bcf",
        "--gpu",
        "--month-rotate",
        "15",
    ];
    let config = AgentConfig::from_args_and_env(args, |key| match key {
        "AGENT_ENDPOINT" => Some("https://env.example.com".to_string()),
        "AGENT_TOKEN" => Some("env-token".to_string()),
        "AGENT_INCLUDE_NICS" => Some("env0".to_string()),
        "AGENT_EXCLUDE_NICS" => Some("env1".to_string()),
        "AGENT_INCLUDE_MOUNTPOINTS" => Some("/env".to_string()),
        "AGENT_CUSTOM_IPV4" => Some("198.51.100.10".to_string()),
        "AGENT_CUSTOM_IPV6" => Some("2607:f358:1a:e::ab0:39b7".to_string()),
        "AGENT_CUSTOM_DNS" => Some("8.8.8.8".to_string()),
        "AGENT_GET_IP_ADDR_FROM_NIC" => Some("false".to_string()),
        "AGENT_MEMORY_INCLUDE_CACHE" => Some("false".to_string()),
        "AGENT_MEMORY_REPORT_RAW_USED" => Some("false".to_string()),
        "AGENT_ENABLE_GPU" => Some("false".to_string()),
        "AGENT_MONTH_ROTATE" => Some("9".to_string()),
        _ => None,
    })
    .unwrap();

    assert_eq!(config.endpoint, "https://env.example.com");
    assert_eq!(config.token, "env-token");
    assert_eq!(config.include_nics, "env0");
    assert_eq!(config.exclude_nics, "env1");
    assert_eq!(config.include_mountpoints, "/env");
    assert_eq!(config.custom_ipv4, "198.51.100.10");
    assert_eq!(config.custom_ipv6, "2607:f358:1a:e::ab0:39b7");
    assert_eq!(config.custom_dns, "8.8.8.8");
    // Go's loadFromEnv only turns bools on for "true"/"1"; "false" leaves CLI true.
    assert!(config.get_ip_addr_from_nic);
    assert!(config.memory_include_cache);
    assert!(config.memory_report_raw_used);
    assert!(config.enable_gpu);
    assert_eq!(config.month_rotate, 9);
}

#[test]
fn config_supports_boolean_flags() {
    let args = [
        "kelicloud-agent-rs",
        "--endpoint",
        "https://cli.example.com",
        "--token",
        "cli-token",
        "--insecure",
        "--disable-web-ssh",
    ];
    let config = AgentConfig::from_args_and_env(args, |_| None).unwrap();

    assert!(config.insecure);
    assert!(config.disable_web_ssh);
}

#[test]
fn config_reads_connection_options_from_environment() {
    let args = ["kelicloud-agent-rs"];
    let config = AgentConfig::from_args_and_env(args, |key| match key {
        "AGENT_ENDPOINT" => Some("https://env.example.com".to_string()),
        "AGENT_TOKEN" => Some("env-token".to_string()),
        "AGENT_INTERVAL" => Some("2.5".to_string()),
        "AGENT_MAX_RETRIES" => Some("8".to_string()),
        "AGENT_RECONNECT_INTERVAL" => Some("13".to_string()),
        "AGENT_INFO_REPORT_INTERVAL" => Some("21".to_string()),
        "AGENT_CF_ACCESS_CLIENT_ID" => Some("cf-id".to_string()),
        "AGENT_CF_ACCESS_CLIENT_SECRET" => Some("cf-secret".to_string()),
        "AGENT_IGNORE_UNSAFE_CERT" => Some("true".to_string()),
        _ => None,
    })
    .unwrap();

    assert_eq!(config.interval_seconds, 2.5);
    assert_eq!(config.max_retries, 8);
    assert_eq!(config.reconnect_interval_seconds, 13);
    assert_eq!(config.info_report_interval_minutes, 21);
    assert_eq!(config.cf_access_client_id, "cf-id");
    assert_eq!(config.cf_access_client_secret, "cf-secret");
    assert!(config.insecure);
}

#[test]
fn config_command_line_overrides_connection_options() {
    let args = [
        "kelicloud-agent-rs",
        "--endpoint",
        "https://cli.example.com",
        "--token",
        "cli-token",
        "--interval",
        "3.5",
        "--max-retries",
        "9",
        "--reconnect-interval",
        "14",
        "--info-report-interval",
        "22",
        "--cf-access-client-id",
        "cli-cf-id",
        "--cf-access-client-secret",
        "cli-cf-secret",
        "--ignore-unsafe-cert",
        "--once",
    ];
    let config = AgentConfig::from_args_and_env(args, |_| None).unwrap();

    assert_eq!(config.interval_seconds, 3.5);
    assert_eq!(config.max_retries, 9);
    assert_eq!(config.reconnect_interval_seconds, 14);
    assert_eq!(config.info_report_interval_minutes, 22);
    assert_eq!(config.cf_access_client_id, "cli-cf-id");
    assert_eq!(config.cf_access_client_secret, "cli-cf-secret");
    assert!(config.insecure);
    assert!(config.once);
}

#[test]
fn config_accepts_zero_info_report_interval_for_every_cycle() {
    let args = [
        "kelicloud-agent-rs",
        "--endpoint",
        "https://cli.example.com",
        "--token",
        "cli-token",
        "--info-report-interval",
        "0",
    ];
    let config = AgentConfig::from_args_and_env(args, |_| None).unwrap();

    assert_eq!(config.info_report_interval_minutes, 0);
}

#[test]
fn config_accepts_go_agent_short_connection_flags() {
    let args = [
        "kelicloud-agent-rs",
        "-e",
        "https://cli.example.com",
        "-t",
        "cli-token",
        "-i",
        "3.5",
        "-u",
        "-r",
        "9",
        "-c",
        "14",
    ];
    let config = AgentConfig::from_args_and_env(args, |_| None).unwrap();

    assert_eq!(config.endpoint, "https://cli.example.com");
    assert_eq!(config.token, "cli-token");
    assert_eq!(config.interval_seconds, 3.5);
    assert!(config.insecure);
    assert_eq!(config.max_retries, 9);
    assert_eq!(config.reconnect_interval_seconds, 14);
}

#[test]
fn config_ignores_unknown_go_agent_flags_like_cobra() {
    let config = AgentConfig::from_args_and_env(
        [
            "kelicloud-agent-rs",
            "--endpoint",
            "https://cli.example.com",
            "--token",
            "cli-token",
            "--disable-auto-update",
            "--show-warning",
            "--future-go-flag=value",
            "positional-arg",
        ],
        |_| None,
    )
    .unwrap();

    assert_eq!(config.endpoint, "https://cli.example.com");
    assert_eq!(config.token, "cli-token");
}

#[test]
fn config_accepts_auto_discovery_key_without_static_token() {
    let config = AgentConfig::from_args_and_env(
        [
            "kelicloud-agent-rs",
            "--endpoint",
            "https://cli.example.com",
            "--auto-discovery",
            "discovery-key",
        ],
        |_| None,
    )
    .unwrap();

    assert_eq!(config.endpoint, "https://cli.example.com");
    assert_eq!(config.token, "");
    assert_eq!(config.auto_discovery_key, "discovery-key");
}

#[test]
fn config_accepts_auto_discovery_key_from_environment() {
    let config = AgentConfig::from_args_and_env(["kelicloud-agent-rs"], |key| match key {
        "AGENT_ENDPOINT" => Some("https://env.example.com".to_string()),
        "AGENT_AUTO_DISCOVERY_KEY" => Some("env-discovery-key".to_string()),
        _ => None,
    })
    .unwrap();

    assert_eq!(config.endpoint, "https://env.example.com");
    assert_eq!(config.token, "");
    assert_eq!(config.auto_discovery_key, "env-discovery-key");
}

#[test]
fn config_supports_go_agent_metric_options() {
    let args = [
        "kelicloud-agent-rs",
        "--endpoint",
        "https://cli.example.com",
        "--token",
        "cli-token",
        "--include-nics",
        "eth0,ens18",
        "--exclude-nics",
        "docker0",
        "--include-mountpoints",
        "/;/data",
        "--custom-ipv4",
        "203.0.113.10",
        "--custom-ipv6",
        "2607:f358:1a:e::ab0:39b7",
        "--get-ip-addr-from-nic",
        "--memory-include-cache",
        "--memory-exclude-bcf",
        "--enable-gpu",
        "--month-rotate",
        "15",
    ];

    let config = AgentConfig::from_args_and_env(args, |_| None).unwrap();

    assert_eq!(config.include_nics, "eth0,ens18");
    assert_eq!(config.exclude_nics, "docker0");
    assert_eq!(config.include_mountpoints, "/;/data");
    assert_eq!(config.custom_ipv4, "203.0.113.10");
    assert_eq!(config.custom_ipv6, "2607:f358:1a:e::ab0:39b7");
    assert!(config.get_ip_addr_from_nic);
    assert!(config.memory_include_cache);
    assert!(config.memory_report_raw_used);
    assert!(config.enable_gpu);
    assert_eq!(config.month_rotate, 15);
}

#[test]
fn config_ignores_deprecated_go_agent_memory_mode_available_flag() {
    for deprecated_flag in ["--memory-mode-available", "-memory-mode-available"] {
        let config = AgentConfig::from_args_and_env(
            [
                "kelicloud-agent-rs",
                "--endpoint",
                "https://cli.example.com",
                "--token",
                "cli-token",
                deprecated_flag,
            ],
            |_| None,
        )
        .unwrap();

        assert!(!config.memory_include_cache);
        assert!(!config.memory_report_raw_used);
    }
}

#[test]
fn config_accepts_go_agent_custom_dns_option() {
    let from_cli = AgentConfig::from_args_and_env(
        [
            "kelicloud-agent-rs",
            "--endpoint",
            "https://cli.example.com",
            "--token",
            "cli-token",
            "--custom-dns",
            "2606:4700:4700::1111",
        ],
        |_| None,
    )
    .unwrap();
    assert_eq!(from_cli.custom_dns, "2606:4700:4700::1111");

    let from_env = AgentConfig::from_args_and_env(["kelicloud-agent-rs"], |key| match key {
        "AGENT_ENDPOINT" => Some("https://env.example.com".to_string()),
        "AGENT_TOKEN" => Some("env-token".to_string()),
        "AGENT_CUSTOM_DNS" => Some("1.1.1.1".to_string()),
        _ => None,
    })
    .unwrap();
    assert_eq!(from_env.custom_dns, "1.1.1.1");
}

#[test]
fn config_accepts_go_agent_gpu_flag_alias() {
    let config = AgentConfig::from_args_and_env(
        [
            "kelicloud-agent-rs",
            "--endpoint",
            "https://cli.example.com",
            "--token",
            "cli-token",
            "--gpu",
        ],
        |_| None,
    )
    .unwrap();

    assert!(config.enable_gpu);
}

#[test]
fn config_accepts_go_agent_include_mountpoint_flag_alias() {
    let config = AgentConfig::from_args_and_env(
        [
            "kelicloud-agent-rs",
            "--endpoint",
            "https://cli.example.com",
            "--token",
            "cli-token",
            "--include-mountpoint",
            "/;/data",
        ],
        |_| None,
    )
    .unwrap();

    assert_eq!(config.include_mountpoints, "/;/data");
}

#[test]
fn config_reads_go_agent_metric_options_from_environment() {
    let args = ["kelicloud-agent-rs"];
    let config = AgentConfig::from_args_and_env(args, |key| match key {
        "AGENT_ENDPOINT" => Some("https://env.example.com".to_string()),
        "AGENT_TOKEN" => Some("env-token".to_string()),
        "AGENT_INCLUDE_NICS" => Some("eth0,ens18".to_string()),
        "AGENT_EXCLUDE_NICS" => Some("docker0".to_string()),
        "AGENT_INCLUDE_MOUNTPOINTS" => Some("/;/data".to_string()),
        "AGENT_CUSTOM_IPV4" => Some("203.0.113.10".to_string()),
        "AGENT_CUSTOM_IPV6" => Some("2607:f358:1a:e::ab0:39b7".to_string()),
        "AGENT_GET_IP_ADDR_FROM_NIC" => Some("true".to_string()),
        "AGENT_MEMORY_INCLUDE_CACHE" => Some("true".to_string()),
        "AGENT_MEMORY_REPORT_RAW_USED" => Some("true".to_string()),
        "AGENT_ENABLE_GPU" => Some("true".to_string()),
        "AGENT_MONTH_ROTATE" => Some("15".to_string()),
        "HOST_PROC" => Some("/host/proc".to_string()),
        _ => None,
    })
    .unwrap();

    assert_eq!(config.include_nics, "eth0,ens18");
    assert_eq!(config.exclude_nics, "docker0");
    assert_eq!(config.include_mountpoints, "/;/data");
    assert_eq!(config.custom_ipv4, "203.0.113.10");
    assert_eq!(config.custom_ipv6, "2607:f358:1a:e::ab0:39b7");
    assert!(config.get_ip_addr_from_nic);
    assert!(config.memory_include_cache);
    assert!(config.memory_report_raw_used);
    assert!(config.enable_gpu);
    assert_eq!(config.month_rotate, 15);
    assert_eq!(config.host_proc, "/host/proc");
}

#[test]
fn config_env_booleans_match_go_agent_true_and_one_only() {
    let config = AgentConfig::from_args_and_env(["kelicloud-agent-rs"], |key| match key {
        "AGENT_ENDPOINT" => Some("https://env.example.com".to_string()),
        "AGENT_TOKEN" => Some("env-token".to_string()),
        "AGENT_IGNORE_UNSAFE_CERT" => Some("yes".to_string()),
        "AGENT_DISABLE_WEB_SSH" => Some("on".to_string()),
        "AGENT_GET_IP_ADDR_FROM_NIC" => Some("y".to_string()),
        "AGENT_MEMORY_INCLUDE_CACHE" => Some("yes".to_string()),
        "AGENT_MEMORY_REPORT_RAW_USED" => Some("on".to_string()),
        "AGENT_ENABLE_GPU" => Some("y".to_string()),
        "AGENT_ONCE" => Some("yes".to_string()),
        _ => None,
    })
    .unwrap();

    assert!(!config.insecure);
    assert!(!config.disable_web_ssh);
    assert!(!config.get_ip_addr_from_nic);
    assert!(!config.memory_include_cache);
    assert!(!config.memory_report_raw_used);
    assert!(!config.enable_gpu);
    assert!(!config.once);
}

#[test]
fn config_invalid_numeric_env_values_match_go_agent_ignore_invalid() {
    let config = AgentConfig::from_args_and_env(["kelicloud-agent-rs"], |key| match key {
        "AGENT_ENDPOINT" => Some("https://env.example.com".to_string()),
        "AGENT_TOKEN" => Some("env-token".to_string()),
        "AGENT_INTERVAL" => Some("not-a-number".to_string()),
        "AGENT_MAX_RETRIES" => Some("not-a-number".to_string()),
        "AGENT_RECONNECT_INTERVAL" => Some("not-a-number".to_string()),
        "AGENT_INFO_REPORT_INTERVAL" => Some("not-a-number".to_string()),
        "AGENT_MONTH_ROTATE" => Some("not-a-number".to_string()),
        _ => None,
    })
    .unwrap();

    assert_eq!(config.interval_seconds, 1.0);
    assert_eq!(config.max_retries, 3);
    assert_eq!(config.reconnect_interval_seconds, 5);
    assert_eq!(config.info_report_interval_minutes, 5);
    assert_eq!(config.month_rotate, 0);
}

#[test]
fn config_file_overrides_args_and_env_like_go_agent_json_config() {
    let path = std::env::temp_dir().join(format!(
        "kelicloud-agent-rs-config-{}.json",
        std::process::id()
    ));
    fs::write(
        &path,
        r#"{
            "endpoint": "https://file.example.com",
            "token": "file-token",
            "ignore_unsafe_cert": true,
            "disable_web_ssh": true,
            "interval": 4.5,
            "max_retries": 11,
            "reconnect_interval": 17,
            "info_report_interval": 23,
            "cf_access_client_id": "file-cf-id",
            "cf_access_client_secret": "file-cf-secret",
            "include_nics": "eth0",
            "exclude_nics": "docker0",
            "include_mountpoints": "/;/data",
            "custom_ipv4": "203.0.113.10",
            "custom_ipv6": "2607:f358:1a:e::ab0:39b7",
            "custom_dns": "1.1.1.1",
            "get_ip_addr_from_nic": true,
            "memory_include_cache": true,
            "memory_report_raw_used": true,
            "enable_gpu": true,
            "month_rotate": 9,
            "host_proc": "/host/proc",
            "auto_discovery_key": "file-discovery-key"
        }"#,
    )
    .unwrap();

    let config = AgentConfig::from_args_and_env(
        [
            "kelicloud-agent-rs",
            "--endpoint",
            "https://cli.example.com",
            "--token",
            "cli-token",
            "--interval",
            "2",
            "--config",
            path.to_str().unwrap(),
        ],
        |key| match key {
            "AGENT_ENDPOINT" => Some("https://env.example.com".to_string()),
            "AGENT_TOKEN" => Some("env-token".to_string()),
            "AGENT_MAX_RETRIES" => Some("3".to_string()),
            _ => None,
        },
    )
    .unwrap();
    fs::remove_file(path).unwrap();

    assert_eq!(config.endpoint, "https://file.example.com");
    assert_eq!(config.token, "file-token");
    assert!(config.insecure);
    assert!(config.disable_web_ssh);
    assert_eq!(config.interval_seconds, 4.5);
    assert_eq!(config.max_retries, 11);
    assert_eq!(config.reconnect_interval_seconds, 17);
    assert_eq!(config.info_report_interval_minutes, 23);
    assert_eq!(config.cf_access_client_id, "file-cf-id");
    assert_eq!(config.cf_access_client_secret, "file-cf-secret");
    assert_eq!(config.include_nics, "eth0");
    assert_eq!(config.exclude_nics, "docker0");
    assert_eq!(config.include_mountpoints, "/;/data");
    assert_eq!(config.custom_ipv4, "203.0.113.10");
    assert_eq!(config.custom_ipv6, "2607:f358:1a:e::ab0:39b7");
    assert_eq!(config.custom_dns, "1.1.1.1");
    assert!(config.get_ip_addr_from_nic);
    assert!(config.memory_include_cache);
    assert!(config.memory_report_raw_used);
    assert!(config.enable_gpu);
    assert_eq!(config.month_rotate, 9);
    assert_eq!(config.host_proc, "/host/proc");
    assert_eq!(config.auto_discovery_key, "file-discovery-key");
}

#[test]
fn config_requires_endpoint() {
    let args = ["kelicloud-agent-rs", "--token", "cli-token"];
    let err = AgentConfig::from_args_and_env(args, |_| None).unwrap_err();

    assert!(matches!(err, ConfigError::MissingEndpoint));
}

#[test]
fn config_requires_token() {
    let args = [
        "kelicloud-agent-rs",
        "--endpoint",
        "https://cli.example.com",
    ];
    let err = AgentConfig::from_args_and_env(args, |_| None).unwrap_err();

    assert!(matches!(err, ConfigError::MissingToken));
}
