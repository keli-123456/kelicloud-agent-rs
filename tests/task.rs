use kelicloud_agent_rs::config::AgentConfig;
use kelicloud_agent_rs::protocol::BackendMessage;
use kelicloud_agent_rs::runtime::ControlMessageHandler;
use kelicloud_agent_rs::task::{
    build_task_result_url, HttpTaskResultUploader, LinuxTaskExecutor, TaskControlMessageHandler,
    TaskExecution, TaskExecutor, TaskResult, TaskResultUploader,
};
use kelicloud_agent_rs::token::SharedAgentToken;
use kelicloud_agent_rs::transport::TransportError;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn task_handler_runs_exec_message_and_uploads_result() {
    let uploaded = Arc::new(Mutex::new(Vec::new()));
    let mut handler = TaskControlMessageHandler::new(
        FixedTaskExecutor::new("root", 0),
        RecordingTaskUploader::new(uploaded.clone()),
        false,
    );

    handler.handle(BackendMessage::Exec {
        task_id: "task-1".to_string(),
        command: "whoami".to_string(),
    });

    assert!(
        wait_for_uploaded_count(&uploaded, 1, Duration::from_secs(1)),
        "task result was not uploaded"
    );
    let uploaded = uploaded.lock().unwrap();
    assert_eq!(
        uploaded.as_slice(),
        [TaskResult {
            task_id: "task-1".to_string(),
            result: "root".to_string(),
            exit_code: 0,
            finished_at: uploaded[0].finished_at.clone(),
        }]
    );
    assert!(!uploaded[0].finished_at.is_empty());
}

#[test]
fn task_handler_returns_before_exec_finishes() {
    let uploaded = Arc::new(Mutex::new(Vec::new()));
    let mut handler = TaskControlMessageHandler::new(
        SlowTaskExecutor::new(Duration::from_millis(150)),
        RecordingTaskUploader::new(uploaded.clone()),
        false,
    );

    let started_at = Instant::now();
    handler.handle(BackendMessage::Exec {
        task_id: "task-slow".to_string(),
        command: "sleep".to_string(),
    });

    assert!(
        started_at.elapsed() < Duration::from_millis(75),
        "handler blocked for {:?}",
        started_at.elapsed()
    );
    assert!(
        wait_for_uploaded_count(&uploaded, 1, Duration::from_secs(1)),
        "task result was not uploaded after background execution"
    );
    assert_eq!(uploaded.lock().unwrap()[0].task_id, "task-slow");
    assert_eq!(uploaded.lock().unwrap()[0].result, "slow-done");
}

#[test]
fn task_handler_uploads_disabled_result_without_running_command() {
    let uploaded = Arc::new(Mutex::new(Vec::new()));
    let executed = Arc::new(Mutex::new(Vec::new()));
    let mut handler = TaskControlMessageHandler::new(
        RecordingTaskExecutor::new(executed.clone()),
        RecordingTaskUploader::new(uploaded.clone()),
        true,
    );

    handler.handle(BackendMessage::Exec {
        task_id: "task-disabled".to_string(),
        command: "whoami".to_string(),
    });

    assert!(
        wait_for_uploaded_count(&uploaded, 1, Duration::from_secs(1)),
        "disabled task result was not uploaded"
    );
    assert!(executed.lock().unwrap().is_empty());
    let uploaded = uploaded.lock().unwrap();
    assert_eq!(uploaded[0].task_id, "task-disabled");
    assert_eq!(uploaded[0].result, "Remote control is disabled.");
    assert_eq!(uploaded[0].exit_code, -1);
}

#[test]
fn task_handler_uploads_no_command_result_before_disabled_check() {
    let uploaded = Arc::new(Mutex::new(Vec::new()));
    let mut handler = TaskControlMessageHandler::new(
        FixedTaskExecutor::new("unused", 0),
        RecordingTaskUploader::new(uploaded.clone()),
        true,
    );

    handler.handle(BackendMessage::Exec {
        task_id: "task-empty".to_string(),
        command: "  ".to_string(),
    });

    assert!(
        wait_for_uploaded_count(&uploaded, 1, Duration::from_secs(1)),
        "empty task result was not uploaded"
    );
    let uploaded = uploaded.lock().unwrap();
    assert_eq!(uploaded[0].task_id, "task-empty");
    assert_eq!(uploaded[0].result, "No command provided");
    assert_eq!(uploaded[0].exit_code, 0);
}

#[test]
fn task_handler_ignores_exec_message_without_task_id() {
    let uploaded = Arc::new(Mutex::new(Vec::new()));
    let mut handler = TaskControlMessageHandler::new(
        FixedTaskExecutor::new("unused", 0),
        RecordingTaskUploader::new(uploaded.clone()),
        false,
    );

    handler.handle(BackendMessage::Exec {
        task_id: String::new(),
        command: "whoami".to_string(),
    });

    assert!(uploaded.lock().unwrap().is_empty());
}

#[test]
fn linux_task_executor_combines_stdout_stderr_and_exit_code() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let execution = LinuxTaskExecutor::default().execute("printf 'out'; printf 'err' >&2; exit 7");

    assert_eq!(execution.result, "out\nerr");
    assert_eq!(execution.exit_code, 7);
}

#[test]
fn linux_task_executor_runs_shebang_script() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let execution = LinuxTaskExecutor::default().execute("#!/bin/sh\nprintf 'script-ok'");

    assert_eq!(execution.result, "script-ok");
    assert_eq!(execution.exit_code, 0);
}

#[test]
fn build_task_result_url_trims_endpoint_and_encodes_token() {
    assert_eq!(
        build_task_result_url("https://panel.example.com/", "secret token").unwrap(),
        "https://panel.example.com/api/clients/task/result?token=secret%20token"
    );
}

#[test]
fn http_task_result_uploader_posts_json_and_access_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let received = Arc::new(Mutex::new(String::new()));
    let received_for_thread = received.clone();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_http_request(&mut stream);
        *received_for_thread.lock().unwrap() = request;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
    });
    let config = test_config(endpoint);
    let uploader =
        HttpTaskResultUploader::from_config_with_retry_delay(&config, Duration::from_millis(0))
            .unwrap();

    uploader
        .upload_task_result(&TaskResult {
            task_id: "task-http".to_string(),
            result: "done".to_string(),
            exit_code: 0,
            finished_at: "2026-06-13T00:00:00Z".to_string(),
        })
        .unwrap();
    server.join().unwrap();

    let request = received.lock().unwrap();
    assert!(request.starts_with("post /api/clients/task/result?token=secret-token-value http/1.1"));
    assert!(request.contains("cf-access-client-id: cf-id"));
    assert!(request.contains("cf-access-client-secret: cf-secret"));
    assert!(request.contains(r#""task_id":"task-http""#));
    assert!(request.contains(r#""result":"done""#));
    assert!(request.contains(r#""exit_code":0"#));
}

#[test]
fn http_task_result_uploader_uses_updated_shared_token() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let received = Arc::new(Mutex::new(Vec::new()));
    let received_for_thread = received.clone();
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_http_request(&mut stream);
            received_for_thread.lock().unwrap().push(request);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        }
    });
    let mut config = test_config(endpoint);
    config.token = "stale-token".to_string();
    let token = SharedAgentToken::new(config.token.clone());
    let uploader = HttpTaskResultUploader::from_config_with_token_and_retry_delay(
        &config,
        token.clone(),
        Duration::from_millis(0),
    )
    .unwrap();

    uploader
        .upload_task_result(&task_result("task-stale", "one"))
        .unwrap();
    token.set("fresh-token");
    uploader
        .upload_task_result(&task_result("task-fresh", "two"))
        .unwrap();
    server.join().unwrap();

    let received = received.lock().unwrap();
    assert!(received[0].starts_with("post /api/clients/task/result?token=stale-token http/1.1"));
    assert!(received[1].starts_with("post /api/clients/task/result?token=fresh-token http/1.1"));
}

struct FixedTaskExecutor {
    result: String,
    exit_code: i32,
}

impl FixedTaskExecutor {
    fn new(result: &str, exit_code: i32) -> Self {
        Self {
            result: result.to_string(),
            exit_code,
        }
    }
}

impl TaskExecutor for FixedTaskExecutor {
    fn execute(&self, _command: &str) -> TaskExecution {
        TaskExecution {
            result: self.result.clone(),
            exit_code: self.exit_code,
        }
    }
}

struct RecordingTaskExecutor {
    commands: Arc<Mutex<Vec<String>>>,
}

impl RecordingTaskExecutor {
    fn new(commands: Arc<Mutex<Vec<String>>>) -> Self {
        Self { commands }
    }
}

impl TaskExecutor for RecordingTaskExecutor {
    fn execute(&self, command: &str) -> TaskExecution {
        self.commands.lock().unwrap().push(command.to_string());
        TaskExecution {
            result: "executed".to_string(),
            exit_code: 0,
        }
    }
}

struct RecordingTaskUploader {
    uploaded: Arc<Mutex<Vec<TaskResult>>>,
}

impl RecordingTaskUploader {
    fn new(uploaded: Arc<Mutex<Vec<TaskResult>>>) -> Self {
        Self { uploaded }
    }
}

impl TaskResultUploader for RecordingTaskUploader {
    fn upload_task_result(&self, result: &TaskResult) -> Result<(), TransportError> {
        self.uploaded.lock().unwrap().push(result.clone());
        Ok(())
    }
}

struct SlowTaskExecutor {
    delay: Duration,
}

impl SlowTaskExecutor {
    fn new(delay: Duration) -> Self {
        Self { delay }
    }
}

impl TaskExecutor for SlowTaskExecutor {
    fn execute(&self, _command: &str) -> TaskExecution {
        thread::sleep(self.delay);
        TaskExecution {
            result: "slow-done".to_string(),
            exit_code: 0,
        }
    }
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
        cf_access_client_id: "cf-id".to_string(),
        cf_access_client_secret: "cf-secret".to_string(),
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

fn wait_for_uploaded_count(
    uploaded: &Arc<Mutex<Vec<TaskResult>>>,
    count: usize,
    timeout: Duration,
) -> bool {
    let started_at = Instant::now();
    while started_at.elapsed() < timeout {
        if uploaded.lock().unwrap().len() >= count {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

fn task_result(task_id: &str, result: &str) -> TaskResult {
    TaskResult {
        task_id: task_id.to_string(),
        result: result.to_string(),
        exit_code: 0,
        finished_at: "2026-06-13T00:00:00Z".to_string(),
    }
}

fn read_http_request(stream: &mut std::net::TcpStream) -> String {
    let mut buffer = Vec::new();
    let mut chunk = [0; 1024];
    loop {
        let count = stream.read(&mut chunk).unwrap();
        buffer.extend_from_slice(&chunk[..count]);
        if let Some(header_end) = find_header_end(&buffer) {
            let headers = String::from_utf8_lossy(&buffer[..header_end]).to_ascii_lowercase();
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            let body_start = header_end + 4;
            while buffer.len() < body_start + content_length {
                let count = stream.read(&mut chunk).unwrap();
                buffer.extend_from_slice(&chunk[..count]);
            }
            break;
        }
    }

    String::from_utf8_lossy(&buffer).to_ascii_lowercase()
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}
