use kelicloud_agent_rs::config::AgentConfig;
use kelicloud_agent_rs::report::BasicInfo;
use kelicloud_agent_rs::transport::{
    access_headers, build_basic_info_url, HttpTransport, ReportSocket, ReqwestHttpTransport,
    TransportError, TungsteniteWebSocketTransport, WebSocketTransport,
};
use kelicloud_agent_rs::tunnel_async_runtime::TunnelRelayBatchPolicy;
use std::io::{Read, Write};
use std::net::{TcpListener, UdpSocket};

fn config_with_cf(id: &str, secret: &str) -> AgentConfig {
    AgentConfig {
        endpoint: "https://panel.example.com".to_string(),
        token: "secret-token-value".to_string(),
        auto_discovery_key: String::new(),
        insecure: false,
        disable_web_ssh: false,
        tunnel_control_enabled: true,
        tunnel_data_enabled: false,
        tunnel_ktp_tcp_address: String::new(),
        tunnel_ktp_relay_batch_policy: TunnelRelayBatchPolicy::Fixed,
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

#[test]
fn basic_info_upload_classifies_go_agent_invalid_token_response() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 2048];
        let _ = stream.read(&mut request).unwrap();
        stream
            .write_all(
                b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 25\r\n\r\n{\"error\":\"invalid token\"}",
            )
            .unwrap();
    });
    let mut transport = ReqwestHttpTransport::new(false).expect("transport");

    let err = transport
        .upload_basic_info(
            &format!(
                "http://127.0.0.1:{}/api/clients/uploadBasicInfo?token=secret-token",
                addr.port()
            ),
            &[],
            &basic_info(),
        )
        .unwrap_err();
    server.join().unwrap();

    assert_eq!(
        err,
        TransportError::InvalidClientToken {
            operation: "upload basic info".to_string(),
            token: "secret-token".to_string(),
            status_code: 401,
            detail: r#"{"error":"invalid token"}"#.to_string(),
        }
    );
}

#[test]
fn websocket_connect_classifies_go_agent_invalid_token_response() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 2048];
        let _ = stream.read(&mut request).unwrap();
        stream
            .write_all(
                b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 25\r\n\r\n{\"error\":\"invalid token\"}",
            )
            .unwrap();
    });
    let mut transport = TungsteniteWebSocketTransport::new();

    let err = match transport.connect_report(
        &format!(
            "ws://127.0.0.1:{}/api/clients/report?token=secret-token",
            addr.port()
        ),
        &[],
    ) {
        Ok(_) => panic!("expected invalid token error"),
        Err(error) => error,
    };
    server.join().unwrap();

    assert_eq!(
        err,
        TransportError::InvalidClientToken {
            operation: "connect websocket".to_string(),
            token: "secret-token".to_string(),
            status_code: 401,
            detail: r#"{"error":"invalid token"}"#.to_string(),
        }
    );
}

#[test]
fn basic_info_upload_keeps_other_unauthorized_responses_generic() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 2048];
        let _ = stream.read(&mut request).unwrap();
        stream
            .write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 13\r\n\r\naccess denied")
            .unwrap();
    });
    let mut transport = ReqwestHttpTransport::new(false).expect("transport");

    let err = transport
        .upload_basic_info(
            &format!(
                "http://127.0.0.1:{}/api/clients/uploadBasicInfo?token=secret-token",
                addr.port()
            ),
            &[],
            &basic_info(),
        )
        .unwrap_err();
    server.join().unwrap();

    assert!(matches!(err, TransportError::RequestFailed(_)));
}

#[test]
fn reqwest_http_transport_uses_custom_dns_for_basic_info_upload() {
    let http_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let http_addr = http_listener.local_addr().unwrap();
    let http_thread = std::thread::spawn(move || {
        let (mut stream, _) = http_listener.accept().unwrap();
        let mut request = [0_u8; 2048];
        let len = stream.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..len]);
        assert!(request.starts_with("POST /api/clients/uploadBasicInfo?token=secret HTTP/1.1"));
        assert!(request
            .to_ascii_lowercase()
            .contains("host: panel.internal.test:"));
        assert!(request.contains(r#""version":"rs-test""#));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
    });

    let dns_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let dns_addr = dns_socket.local_addr().unwrap();
    let dns_thread = std::thread::spawn(move || {
        for _ in 0..2 {
            let mut request = [0_u8; 512];
            let (len, peer) = dns_socket.recv_from(&mut request).unwrap();
            let mut response = Vec::new();
            response.extend_from_slice(&request[0..2]);
            response.extend_from_slice(&[0x81, 0x80]);
            response.extend_from_slice(&[0x00, 0x01]);
            let qtype = u16::from_be_bytes([request[len - 4], request[len - 3]]);
            let answer_count = if qtype == 1 { 1_u16 } else { 0_u16 };
            response.extend_from_slice(&answer_count.to_be_bytes());
            response.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
            response.extend_from_slice(&request[12..len]);
            if qtype == 1 {
                response.extend_from_slice(&[0xC0, 0x0C]);
                response.extend_from_slice(&[0x00, 0x01]);
                response.extend_from_slice(&[0x00, 0x01]);
                response.extend_from_slice(&[0x00, 0x00, 0x00, 0x3C]);
                response.extend_from_slice(&[0x00, 0x04]);
                response.extend_from_slice(&[127, 0, 0, 1]);
            }
            dns_socket.send_to(&response, peer).unwrap();
        }
    });

    let mut transport =
        ReqwestHttpTransport::new_with_custom_dns(false, &dns_addr.to_string()).expect("transport");
    transport
        .upload_basic_info(
            &format!(
                "http://panel.internal.test:{}/api/clients/uploadBasicInfo?token=secret",
                http_addr.port()
            ),
            &[],
            &basic_info(),
        )
        .unwrap();

    http_thread.join().unwrap();
    dns_thread.join().unwrap();
}

#[test]
fn tungstenite_websocket_transport_uses_custom_dns_for_report_connection() {
    let ws_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let ws_addr = ws_listener.local_addr().unwrap();
    let ws_thread = std::thread::spawn(move || {
        let (stream, _) = ws_listener.accept().unwrap();
        let mut websocket = tungstenite::accept(stream).unwrap();
        let message = websocket.read().unwrap();
        assert!(message.to_text().unwrap().contains(r#""process":42"#));
    });

    let dns_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let dns_addr = dns_socket.local_addr().unwrap();
    let dns_thread = std::thread::spawn(move || {
        for _ in 0..2 {
            let mut request = [0_u8; 512];
            let (len, peer) = dns_socket.recv_from(&mut request).unwrap();
            let mut response = Vec::new();
            response.extend_from_slice(&request[0..2]);
            response.extend_from_slice(&[0x81, 0x80]);
            response.extend_from_slice(&[0x00, 0x01]);
            let qtype = u16::from_be_bytes([request[len - 4], request[len - 3]]);
            let answer_count = if qtype == 1 { 1_u16 } else { 0_u16 };
            response.extend_from_slice(&answer_count.to_be_bytes());
            response.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
            response.extend_from_slice(&request[12..len]);
            if qtype == 1 {
                response.extend_from_slice(&[0xC0, 0x0C]);
                response.extend_from_slice(&[0x00, 0x01]);
                response.extend_from_slice(&[0x00, 0x01]);
                response.extend_from_slice(&[0x00, 0x00, 0x00, 0x3C]);
                response.extend_from_slice(&[0x00, 0x04]);
                response.extend_from_slice(&[127, 0, 0, 1]);
            }
            dns_socket.send_to(&response, peer).unwrap();
        }
    });

    let mut transport = TungsteniteWebSocketTransport::new_with_custom_dns(&dns_addr.to_string());
    let mut socket = transport
        .connect_report(
            &format!(
                "ws://panel.internal.test:{}/api/clients/report?token=secret",
                ws_addr.port()
            ),
            &[],
        )
        .unwrap();
    socket.send_report(&report()).unwrap();

    ws_thread.join().unwrap();
    dns_thread.join().unwrap();
}

fn basic_info() -> BasicInfo {
    BasicInfo {
        cpu_name: "AMD EPYC".to_string(),
        cpu_cores: 4,
        arch: "amd64".to_string(),
        os: "Debian GNU/Linux 12".to_string(),
        kernel_version: "6.8.0".to_string(),
        ipv4: "203.0.113.10".to_string(),
        ipv6: String::new(),
        mem_total: 8192,
        swap_total: 1024,
        disk_total: 100_000,
        gpu_name: String::new(),
        virtualization: "kvm".to_string(),
        version: "rs-test".to_string(),
    }
}

fn report() -> kelicloud_agent_rs::report::Report {
    use kelicloud_agent_rs::report::{
        ConnectionsReport, CpuReport, DiskReport, LoadReport, MemoryReport, NetworkReport, Report,
    };

    Report {
        cpu: CpuReport { usage: 0.001 },
        ram: MemoryReport { total: 1, used: 0 },
        swap: MemoryReport { total: 0, used: 0 },
        load: LoadReport {
            load1: 0.0,
            load5: 0.0,
            load15: 0.0,
        },
        disk: DiskReport { total: 1, used: 0 },
        network: NetworkReport {
            up: 0,
            down: 0,
            total_up: 0,
            total_down: 0,
        },
        connections: ConnectionsReport { tcp: 0, udp: 0 },
        uptime: 0,
        process: 42,
        gpu: None,
        cn_connectivity: None,
        message: String::new(),
    }
}
