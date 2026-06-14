use kelicloud_agent_rs::config::AgentConfig;
use kelicloud_agent_rs::protocol::BackendMessage;
use kelicloud_agent_rs::runtime::ControlMessageHandler;
use kelicloud_agent_rs::terminal::{
    parse_terminal_client_text, terminal_input_for_pty, terminal_input_received_event,
    terminal_output_sent_event, terminal_session_error_event, TerminalClientCommand,
    TerminalConnector, TerminalControlMessageHandler, TerminalError, TungsteniteTerminalConnector,
};
use kelicloud_agent_rs::token::SharedAgentToken;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn terminal_handler_starts_connector_in_background() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut handler = TerminalControlMessageHandler::new(
        SlowTerminalConnector::new(calls.clone(), Duration::from_millis(150)),
        false,
    );

    let started_at = Instant::now();
    handler.handle(BackendMessage::Terminal {
        request_id: "term-1".to_string(),
    });

    assert!(
        started_at.elapsed() < Duration::from_millis(75),
        "terminal handler blocked for {:?}",
        started_at.elapsed()
    );
    assert!(
        wait_for_call_count(&calls, 1, Duration::from_secs(1)),
        "terminal connector was not started"
    );
    assert_eq!(
        calls.lock().unwrap().as_slice(),
        [("term-1".to_string(), false)]
    );
}

#[test]
fn terminal_handler_passes_disabled_flag_to_connector() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut handler =
        TerminalControlMessageHandler::new(RecordingTerminalConnector::new(calls.clone()), true);

    handler.handle(BackendMessage::Terminal {
        request_id: "term-disabled".to_string(),
    });

    assert!(
        wait_for_call_count(&calls, 1, Duration::from_secs(1)),
        "terminal connector was not started"
    );
    assert_eq!(
        calls.lock().unwrap().as_slice(),
        [("term-disabled".to_string(), true)]
    );
}

#[test]
fn terminal_handler_ignores_empty_request_id() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut handler =
        TerminalControlMessageHandler::new(RecordingTerminalConnector::new(calls.clone()), false);

    handler.handle(BackendMessage::Terminal {
        request_id: String::new(),
    });

    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn terminal_session_error_event_reports_request_and_error() {
    assert_eq!(
        terminal_session_error_event("term-1", "connection refused"),
        "smoke: terminal_session_error request_id=term-1 error=connection refused"
    );
}

#[test]
fn terminal_io_events_report_byte_counts() {
    assert_eq!(
        terminal_input_received_event(7),
        "smoke: terminal_input_received bytes=7"
    );
    assert_eq!(
        terminal_output_sent_event(11),
        "smoke: terminal_output_sent bytes=11"
    );
}

#[test]
fn parse_terminal_client_text_extracts_input_command() {
    assert_eq!(
        parse_terminal_client_text(br#"{"type":"input","input":"whoami\n"}"#),
        TerminalClientCommand::Input(b"whoami\n".to_vec())
    );
}

#[test]
fn parse_terminal_client_text_extracts_resize_command() {
    assert_eq!(
        parse_terminal_client_text(br#"{"type":"resize","cols":120,"rows":40}"#),
        TerminalClientCommand::Resize {
            cols: 120,
            rows: 40
        }
    );
}

#[test]
fn parse_terminal_client_text_treats_plain_text_as_input() {
    assert_eq!(
        parse_terminal_client_text(b"raw input"),
        TerminalClientCommand::Input(b"raw input".to_vec())
    );
}

#[test]
fn terminal_input_for_pty_converts_xterm_carriage_return_to_newline() {
    assert_eq!(
        terminal_input_for_pty(b"printf 'ok\\n'\r"),
        b"printf 'ok\\n'\n".to_vec()
    );
}

#[test]
fn tungstenite_terminal_connector_uses_updated_shared_token() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let requests_for_thread = requests.clone();
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_ws_handshake(&mut stream);
            requests_for_thread.lock().unwrap().push(request);
            stream
                .write_all(
                    b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        }
    });
    let mut config = test_config(endpoint);
    config.token = "stale-token".to_string();
    let token = SharedAgentToken::new(config.token.clone());
    let connector = TungsteniteTerminalConnector::from_config_with_token(&config, token.clone());

    let _ = connector.start_terminal("term-stale", true);
    token.set("fresh-token");
    let _ = connector.start_terminal("term-fresh", true);
    server.join().unwrap();

    let requests = requests.lock().unwrap();
    assert!(requests[0]
        .starts_with("get /api/clients/terminal?token=stale-token&id=term-stale http/1.1"));
    assert!(requests[1]
        .starts_with("get /api/clients/terminal?token=fresh-token&id=term-fresh http/1.1"));
}

#[derive(Clone)]
struct RecordingTerminalConnector {
    calls: Arc<Mutex<Vec<(String, bool)>>>,
}

impl RecordingTerminalConnector {
    fn new(calls: Arc<Mutex<Vec<(String, bool)>>>) -> Self {
        Self { calls }
    }
}

impl TerminalConnector for RecordingTerminalConnector {
    fn start_terminal(
        &self,
        request_id: &str,
        remote_control_disabled: bool,
    ) -> Result<(), TerminalError> {
        self.calls
            .lock()
            .unwrap()
            .push((request_id.to_string(), remote_control_disabled));
        Ok(())
    }
}

#[derive(Clone)]
struct SlowTerminalConnector {
    calls: Arc<Mutex<Vec<(String, bool)>>>,
    delay: Duration,
}

impl SlowTerminalConnector {
    fn new(calls: Arc<Mutex<Vec<(String, bool)>>>, delay: Duration) -> Self {
        Self { calls, delay }
    }
}

impl TerminalConnector for SlowTerminalConnector {
    fn start_terminal(
        &self,
        request_id: &str,
        remote_control_disabled: bool,
    ) -> Result<(), TerminalError> {
        thread::sleep(self.delay);
        self.calls
            .lock()
            .unwrap()
            .push((request_id.to_string(), remote_control_disabled));
        Ok(())
    }
}

fn wait_for_call_count(
    calls: &Arc<Mutex<Vec<(String, bool)>>>,
    count: usize,
    timeout: Duration,
) -> bool {
    let started_at = Instant::now();
    while started_at.elapsed() < timeout {
        if calls.lock().unwrap().len() >= count {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

fn read_ws_handshake(stream: &mut std::net::TcpStream) -> String {
    let mut buffer = Vec::new();
    let mut chunk = [0; 1024];
    loop {
        let count = stream.read(&mut chunk).unwrap();
        if count == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..count]);
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }

    String::from_utf8_lossy(&buffer).to_ascii_lowercase()
}

fn test_config(endpoint: String) -> AgentConfig {
    AgentConfig {
        endpoint,
        token: "secret-token-value".to_string(),
        auto_discovery_key: String::new(),
        insecure: true,
        disable_web_ssh: false,
        interval_seconds: 1.0,
        max_retries: 0,
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
