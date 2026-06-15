use kelicloud_agent_rs::auto_discovery::{
    auto_discovery_registered_smoke_line, build_auto_discovery_register_url,
    resolve_auto_discovery_with, AutoDiscoveryCache, AutoDiscoveryError,
    AutoDiscoveryRegisterRequest, AutoDiscoveryRegistrar, AutoDiscoveryTokenRecovery,
    ReqwestAutoDiscoveryRegistrar,
};
use kelicloud_agent_rs::config::AgentConfig;
use kelicloud_agent_rs::transport::TransportError;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[test]
fn register_url_normalizes_base_and_escapes_hostname() {
    let url =
        build_auto_discovery_register_url("https://panel.example.com/base/", "Auto node/slash")
            .unwrap();

    assert_eq!(
        url,
        "https://panel.example.com/base/api/clients/register?name=Auto%20node%2Fslash"
    );
}

#[test]
fn auto_discovery_registered_smoke_line_includes_uuid_only() {
    let cache = AutoDiscoveryCache {
        uuid: "registered-client".to_string(),
        token: "registered-token".to_string(),
    };

    assert_eq!(
        auto_discovery_registered_smoke_line(&cache),
        "smoke: auto_discovery_registered uuid=registered-client"
    );
}

#[test]
fn resolve_uses_cached_token_without_registering() {
    let cache_path = temp_cache_path("cached");
    fs::write(
        &cache_path,
        r#"{"uuid":"cached-client","token":"cached-token"}"#,
    )
    .unwrap();
    let mut config = auto_config();
    let mut registrar = RecordingRegistrar::new(AutoDiscoveryCache {
        uuid: "registered-client".to_string(),
        token: "registered-token".to_string(),
    });

    let registered =
        resolve_auto_discovery_with(&mut config, &cache_path, "Auto node", &mut registrar).unwrap();

    fs::remove_file(&cache_path).unwrap();
    assert!(!registered);
    assert_eq!(config.token, "cached-token");
    assert!(registrar.requests.is_empty());
}

#[test]
fn resolve_registers_and_saves_token_when_cache_is_missing() {
    let cache_path = temp_cache_path("missing");
    let _ = fs::remove_file(&cache_path);
    let mut config = auto_config();
    config.cf_access_client_id = "cf-id".to_string();
    config.cf_access_client_secret = "cf-secret".to_string();
    let mut registrar = RecordingRegistrar::new(AutoDiscoveryCache {
        uuid: "registered-client".to_string(),
        token: "registered-token".to_string(),
    });

    let registered =
        resolve_auto_discovery_with(&mut config, &cache_path, "Auto node", &mut registrar).unwrap();

    let saved = fs::read_to_string(&cache_path).unwrap();
    fs::remove_file(&cache_path).unwrap();
    assert!(registered);
    assert_eq!(config.token, "registered-token");
    assert_eq!(registrar.requests.len(), 1);
    assert_eq!(
        registrar.requests[0].url,
        "http://panel.example.com/api/clients/register?name=Auto%20node"
    );
    assert_eq!(registrar.requests[0].key, "discovery-key");
    assert_eq!(
        registrar.requests[0].headers,
        vec![
            ("CF-Access-Client-Id".to_string(), "cf-id".to_string()),
            (
                "CF-Access-Client-Secret".to_string(),
                "cf-secret".to_string()
            )
        ]
    );
    assert!(saved.contains(r#""uuid":"registered-client""#));
    assert!(saved.contains(r#""token":"registered-token""#));
}

#[test]
fn reqwest_registrar_posts_go_agent_compatible_register_request() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 4096];
        let len = stream.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..len]);
        let lower = request.to_ascii_lowercase();
        assert!(request.starts_with("POST /base/api/clients/register?name=Auto%20node HTTP/1.1"));
        assert!(lower.contains("authorization: bearer discovery-key"));
        assert!(lower.contains("content-type: application/json"));
        assert!(lower.contains("cf-access-client-id: cf-id"));
        assert!(lower.contains("cf-access-client-secret: cf-secret"));
        assert!(request.contains(r#""key":"discovery-key""#));
        let body =
            br#"{"status":"success","message":"ok","data":{"uuid":"server-client","token":"server-token"}}"#;
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
    });

    let cache_path = temp_cache_path("reqwest");
    let _ = fs::remove_file(&cache_path);
    let mut config = auto_config();
    config.endpoint = format!("http://127.0.0.1:{}/base", addr.port());
    config.cf_access_client_id = "cf-id".to_string();
    config.cf_access_client_secret = "cf-secret".to_string();
    let mut registrar = ReqwestAutoDiscoveryRegistrar::from_config(&config).unwrap();

    let registered =
        resolve_auto_discovery_with(&mut config, &cache_path, "Auto node", &mut registrar).unwrap();

    server.join().unwrap();
    let saved = fs::read_to_string(&cache_path).unwrap();
    fs::remove_file(&cache_path).unwrap();
    assert!(registered);
    assert_eq!(config.token, "server-token");
    assert!(saved.contains(r#""uuid":"server-client""#));
    assert!(saved.contains(r#""token":"server-token""#));
}

#[test]
fn token_recovery_clears_stale_cache_and_registers_new_token() {
    let cache_path = temp_cache_path("stale");
    fs::write(
        &cache_path,
        r#"{"uuid":"stale-client","token":"stale-token"}"#,
    )
    .unwrap();
    let mut config = auto_config();
    config.token = "stale-token".to_string();
    let registrar = RecordingRegistrar::new(AutoDiscoveryCache {
        uuid: "fresh-client".to_string(),
        token: "fresh-token".to_string(),
    });
    let mut recovery =
        AutoDiscoveryTokenRecovery::new(cache_path.clone(), "Auto node".to_string(), registrar);

    let recovered = recovery.recover_from_transport_error(
        &mut config,
        &invalid_token_error("uploadBasicInfo", "stale-token"),
    );

    let saved = fs::read_to_string(&cache_path).unwrap();
    fs::remove_file(&cache_path).unwrap();
    assert!(recovered);
    assert_eq!(config.token, "fresh-token");
    assert!(saved.contains(r#""uuid":"fresh-client""#));
    assert!(saved.contains(r#""token":"fresh-token""#));
}

#[test]
fn token_recovery_skips_registration_when_token_already_rotated() {
    let cache_path = temp_cache_path("already-rotated");
    fs::write(
        &cache_path,
        r#"{"uuid":"fresh-client","token":"fresh-token"}"#,
    )
    .unwrap();
    let mut config = auto_config();
    config.token = "fresh-token".to_string();
    let registrar = RecordingRegistrar::new(AutoDiscoveryCache {
        uuid: "new-client".to_string(),
        token: "new-token".to_string(),
    });
    let mut recovery =
        AutoDiscoveryTokenRecovery::new(cache_path.clone(), "Auto node".to_string(), registrar);

    let recovered = recovery.recover_from_transport_error(
        &mut config,
        &invalid_token_error("report websocket", "stale-token"),
    );

    let saved = fs::read_to_string(&cache_path).unwrap();
    fs::remove_file(&cache_path).unwrap();
    assert!(recovered);
    assert_eq!(config.token, "fresh-token");
    assert!(saved.contains(r#""token":"fresh-token""#));
    assert!(!saved.contains("new-token"));
}

struct RecordingRegistrar {
    requests: Vec<AutoDiscoveryRegisterRequest>,
    response: AutoDiscoveryCache,
}

impl RecordingRegistrar {
    fn new(response: AutoDiscoveryCache) -> Self {
        Self {
            requests: Vec::new(),
            response,
        }
    }
}

impl AutoDiscoveryRegistrar for RecordingRegistrar {
    fn register(
        &mut self,
        request: AutoDiscoveryRegisterRequest,
    ) -> Result<AutoDiscoveryCache, AutoDiscoveryError> {
        self.requests.push(request);
        Ok(self.response.clone())
    }
}

fn auto_config() -> AgentConfig {
    AgentConfig {
        endpoint: "http://panel.example.com".to_string(),
        token: String::new(),
        auto_discovery_key: "discovery-key".to_string(),
        insecure: false,
        disable_web_ssh: false,
        tunnel_control_enabled: true,
        tunnel_data_enabled: false,
        interval_seconds: 1.0,
        max_retries: 3,
        reconnect_interval_seconds: 5,
        info_report_interval_minutes: 5,
        cf_access_client_id: String::new(),
        cf_access_client_secret: String::new(),
        include_nics: String::new(),
        exclude_nics: String::new(),
        include_mountpoints: String::new(),
        custom_ipv4: String::new(),
        custom_ipv6: String::new(),
        custom_dns: String::new(),
        get_ip_addr_from_nic: false,
        memory_include_cache: false,
        memory_report_raw_used: false,
        enable_gpu: false,
        month_rotate: 0,
        host_proc: String::new(),
        once: false,
    }
}

fn temp_cache_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "kelicloud-agent-rs-auto-discovery-{label}-{}-{}.json",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::SeqCst)
    ))
}

fn invalid_token_error(operation: &str, token: &str) -> TransportError {
    TransportError::InvalidClientToken {
        operation: operation.to_string(),
        token: token.to_string(),
        status_code: 401,
        detail: "invalid token".to_string(),
    }
}
