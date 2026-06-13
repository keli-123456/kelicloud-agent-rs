use kelicloud_agent_rs::config::{AgentConfig, ConfigError};

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
    assert!(!config.once);
}

#[test]
fn config_command_line_overrides_environment() {
    let args = [
        "kelicloud-agent-rs",
        "--endpoint",
        "https://cli.example.com",
        "--token",
        "cli-token",
    ];
    let config = AgentConfig::from_args_and_env(args, env_lookup).unwrap();

    assert_eq!(config.endpoint, "https://cli.example.com");
    assert_eq!(config.token, "cli-token");
    assert!(config.insecure);
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
