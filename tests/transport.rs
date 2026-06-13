use kelicloud_agent_rs::config::AgentConfig;
use kelicloud_agent_rs::transport::{access_headers, build_basic_info_url};

fn config_with_cf(id: &str, secret: &str) -> AgentConfig {
    AgentConfig {
        endpoint: "https://panel.example.com".to_string(),
        token: "secret-token-value".to_string(),
        insecure: false,
        disable_web_ssh: false,
        interval_seconds: 1.0,
        max_retries: 3,
        reconnect_interval_seconds: 5,
        info_report_interval_minutes: 5,
        cf_access_client_id: id.to_string(),
        cf_access_client_secret: secret.to_string(),
        include_nics: String::new(),
        exclude_nics: String::new(),
        include_mountpoints: String::new(),
        custom_ipv4: String::new(),
        custom_ipv6: String::new(),
        get_ip_addr_from_nic: false,
        memory_include_cache: false,
        memory_report_raw_used: false,
        enable_gpu: false,
        month_rotate: 0,
        host_proc: String::new(),
        once: false,
    }
}

#[test]
fn basic_info_url_keeps_http_scheme_and_escapes_token() {
    let url = build_basic_info_url("https://panel.example.com/base/", "token with/slash").unwrap();

    assert_eq!(
        url,
        "https://panel.example.com/base/api/clients/uploadBasicInfo?token=token%20with%2Fslash"
    );
}

#[test]
fn access_headers_include_cloudflare_pair_when_both_values_are_present() {
    let headers = access_headers(&config_with_cf("cf-id", "cf-secret"));

    assert_eq!(
        headers,
        vec![
            ("CF-Access-Client-Id".to_string(), "cf-id".to_string()),
            (
                "CF-Access-Client-Secret".to_string(),
                "cf-secret".to_string()
            )
        ]
    );
}

#[test]
fn access_headers_are_empty_when_cloudflare_pair_is_incomplete() {
    assert!(access_headers(&config_with_cf("cf-id", "")).is_empty());
    assert!(access_headers(&config_with_cf("", "cf-secret")).is_empty());
}
