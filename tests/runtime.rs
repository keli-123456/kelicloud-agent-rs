use kelicloud_agent_rs::config::AgentConfig;
use kelicloud_agent_rs::ping::{FixedPingExecutor, PingResult};
use kelicloud_agent_rs::protocol::BackendMessage;
use kelicloud_agent_rs::report::{
    BasicInfo, ConnectionsReport, CpuReport, DiskReport, LoadReport, MemoryReport, NetworkReport,
    Report, ReportGenerator,
};
use kelicloud_agent_rs::runtime::{
    run_once, run_once_with_ping, run_report_cycles, run_report_cycles_with_delay,
    run_report_cycles_with_ping_delay, startup_summary, ChainControlMessageHandler,
    ControlMessageHandler, LoopDelay,
};
use kelicloud_agent_rs::transport::{
    HeaderPair, HttpTransport, ReportSocket, TransportError, WebSocketTransport,
};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

#[test]
fn startup_summary_redacts_token() {
    let config = AgentConfig {
        endpoint: "https://panel.example.com".to_string(),
        token: "secret-token-value".to_string(),
        insecure: true,
        disable_web_ssh: true,
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
    };

    let summary = startup_summary(&config);

    assert!(summary.contains("https://panel.example.com"));
    assert!(summary.contains("secr...alue"));
    assert!(summary.contains("insecure tls: enabled"));
    assert!(summary.contains("web ssh: disabled"));
    assert!(!summary.contains("secret-token-value"));
}

#[test]
fn run_once_uploads_basic_info_before_connecting_websocket() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut http = FakeHttp::new(events.clone());
    let mut websocket = FakeWebSocketTransport::new(events.clone(), None);
    let mut handler = RecordingHandler::default();

    run_once(
        &test_config(),
        &test_basic_info(),
        &FixedReportGenerator(test_report(22.0)),
        &mut http,
        &mut websocket,
        &mut handler,
    )
    .unwrap();

    assert_eq!(
        events.borrow().as_slice(),
        [
            "upload:https://panel.example.com/api/clients/uploadBasicInfo?token=secret-token-value",
            "connect:wss://panel.example.com/api/clients/report?token=secret-token-value",
            "send_report"
        ]
    );
}

#[test]
fn run_once_retries_basic_info_without_kernel_version_for_legacy_backend() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let uploaded_kernel_versions = Rc::new(RefCell::new(Vec::new()));
    let mut http = FakeHttp::new(events.clone())
        .with_upload_failures(1)
        .with_uploaded_kernel_versions(uploaded_kernel_versions.clone());
    let mut websocket = FakeWebSocketTransport::new(events.clone(), None);
    let mut handler = RecordingHandler::default();

    run_once(
        &test_config(),
        &test_basic_info(),
        &FixedReportGenerator(test_report(22.0)),
        &mut http,
        &mut websocket,
        &mut handler,
    )
    .unwrap();

    assert_eq!(
        uploaded_kernel_versions.borrow().as_slice(),
        ["6.8".to_string(), String::new()]
    );
    assert_eq!(
        events.borrow().as_slice(),
        [
            "upload:https://panel.example.com/api/clients/uploadBasicInfo?token=secret-token-value",
            "upload_error",
            "upload:https://panel.example.com/api/clients/uploadBasicInfo?token=secret-token-value",
            "connect:wss://panel.example.com/api/clients/report?token=secret-token-value",
            "send_report"
        ]
    );
}

#[test]
fn run_once_sends_immediate_report_after_connecting() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let sent_reports = Rc::new(RefCell::new(Vec::new()));
    let mut http = FakeHttp::new(events.clone());
    let mut websocket =
        FakeWebSocketTransport::new(events, None).with_sent_reports(sent_reports.clone());
    let mut handler = RecordingHandler::default();

    run_once(
        &test_config(),
        &test_basic_info(),
        &FixedReportGenerator(test_report(44.0)),
        &mut http,
        &mut websocket,
        &mut handler,
    )
    .unwrap();

    assert_eq!(sent_reports.borrow()[0].cpu.usage, 44.0);
}

#[test]
fn run_once_dispatches_parsed_backend_message() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let inbound =
        br#"{"message":"ping","ping_task_id":7,"ping_type":"tcp","ping_target":"1.1.1.1:443"}"#
            .to_vec();
    let mut http = FakeHttp::new(events.clone());
    let mut websocket = FakeWebSocketTransport::new(events, Some(inbound));
    let mut handler = RecordingHandler::default();

    run_once(
        &test_config(),
        &test_basic_info(),
        &FixedReportGenerator(test_report(12.0)),
        &mut http,
        &mut websocket,
        &mut handler,
    )
    .unwrap();

    assert_eq!(
        handler.messages,
        vec![BackendMessage::Ping {
            task_id: 7,
            ping_type: "tcp".to_string(),
            target: "1.1.1.1:443".to_string(),
        }]
    );
}

#[test]
fn run_once_with_ping_executes_ping_message_and_sends_result() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let sent_ping_results = Rc::new(RefCell::new(Vec::new()));
    let inbound =
        br#"{"message":"ping","ping_task_id":7,"ping_type":"tcp","ping_target":"1.1.1.1:443"}"#
            .to_vec();
    let mut http = FakeHttp::new(events.clone());
    let mut websocket = FakeWebSocketTransport::new(events.clone(), Some(inbound))
        .with_sent_ping_results(sent_ping_results.clone());
    let mut handler = RecordingHandler::default();

    run_once_with_ping(
        &test_config(),
        &test_basic_info(),
        &FixedReportGenerator(test_report(12.0)),
        &FixedPingExecutor::new(29),
        &mut http,
        &mut websocket,
        &mut handler,
    )
    .unwrap();

    assert_eq!(
        events.borrow().as_slice(),
        [
            "upload:https://panel.example.com/api/clients/uploadBasicInfo?token=secret-token-value",
            "connect:wss://panel.example.com/api/clients/report?token=secret-token-value",
            "send_report",
            "send_ping_result"
        ]
    );
    assert_eq!(sent_ping_results.borrow()[0].task_id, 7);
    assert_eq!(sent_ping_results.borrow()[0].ping_type, "tcp");
    assert_eq!(sent_ping_results.borrow()[0].value, 29);
    assert!(handler.messages.is_empty());
}

#[test]
fn chain_control_message_handler_forwards_messages_to_both_handlers() {
    let mut handler =
        ChainControlMessageHandler::new(RecordingHandler::default(), RecordingHandler::default());

    handler.handle(BackendMessage::Exec {
        task_id: "task-1".to_string(),
        command: "uptime".to_string(),
    });

    assert_eq!(
        handler.first().messages,
        vec![BackendMessage::Exec {
            task_id: "task-1".to_string(),
            command: "uptime".to_string(),
        }]
    );
    assert_eq!(
        handler.second().messages,
        vec![BackendMessage::Exec {
            task_id: "task-1".to_string(),
            command: "uptime".to_string(),
        }]
    );
}

#[test]
fn run_report_cycles_sends_reports_and_heartbeat_on_schedule() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut config = test_config();
    config.interval_seconds = 11.0;
    let mut http = FakeHttp::new(events.clone());
    let mut websocket = FakeWebSocketTransport::new(events.clone(), None);
    let mut handler = RecordingHandler::default();

    run_report_cycles(
        &config,
        &test_basic_info(),
        &FixedReportGenerator(test_report(18.0)),
        &mut http,
        &mut websocket,
        &mut handler,
        3,
    )
    .unwrap();

    assert_eq!(
        events.borrow().as_slice(),
        [
            "upload:https://panel.example.com/api/clients/uploadBasicInfo?token=secret-token-value",
            "connect:wss://panel.example.com/api/clients/report?token=secret-token-value",
            "send_report",
            "send_report",
            "send_report",
            "send_ping"
        ]
    );
}

#[test]
fn run_report_cycles_reconnects_after_send_failure() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let sent_reports = Rc::new(RefCell::new(Vec::new()));
    let mut http = FakeHttp::new(events.clone());
    let mut websocket = FakeWebSocketTransport::new(events.clone(), None)
        .with_send_failures(1)
        .with_sent_reports(sent_reports.clone());
    let mut handler = RecordingHandler::default();

    run_report_cycles(
        &test_config(),
        &test_basic_info(),
        &FixedReportGenerator(test_report(31.0)),
        &mut http,
        &mut websocket,
        &mut handler,
        2,
    )
    .unwrap();

    assert_eq!(
        events.borrow().as_slice(),
        [
            "upload:https://panel.example.com/api/clients/uploadBasicInfo?token=secret-token-value",
            "connect:wss://panel.example.com/api/clients/report?token=secret-token-value",
            "send_report_error",
            "connect:wss://panel.example.com/api/clients/report?token=secret-token-value",
            "send_report"
        ]
    );
    assert_eq!(sent_reports.borrow().len(), 1);
}

#[test]
fn run_report_cycles_refreshes_basic_info_on_info_report_interval() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let uploaded_kernel_versions = Rc::new(RefCell::new(Vec::new()));
    let mut config = test_config();
    config.interval_seconds = 61.0;
    config.info_report_interval_minutes = 1;
    let mut http = FakeHttp::new(events.clone())
        .with_uploaded_kernel_versions(uploaded_kernel_versions.clone());
    let mut websocket = FakeWebSocketTransport::new(events.clone(), None);
    let mut handler = RecordingHandler::default();
    let rotating_provider = RotatingBasicInfoProvider::new(["6.8-a", "6.8-b"]);
    let provider = || rotating_provider.basic_info();

    run_report_cycles(
        &config,
        &provider,
        &FixedReportGenerator(test_report(31.0)),
        &mut http,
        &mut websocket,
        &mut handler,
        2,
    )
    .unwrap();

    assert_eq!(
        uploaded_kernel_versions.borrow().as_slice(),
        ["6.8-a".to_string(), "6.8-b".to_string()]
    );
    assert_eq!(
        events.borrow().as_slice(),
        [
            "upload:https://panel.example.com/api/clients/uploadBasicInfo?token=secret-token-value",
            "connect:wss://panel.example.com/api/clients/report?token=secret-token-value",
            "send_report",
            "send_ping",
            "upload:https://panel.example.com/api/clients/uploadBasicInfo?token=secret-token-value",
            "send_report",
            "send_ping",
        ]
    );
}

#[test]
fn run_report_cycles_keeps_reporting_when_periodic_basic_info_refresh_fails() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut config = test_config();
    config.interval_seconds = 61.0;
    config.info_report_interval_minutes = 1;
    let mut http = FakeHttp::new(events.clone()).with_upload_failures_after_successes(1, 2);
    let mut websocket = FakeWebSocketTransport::new(events.clone(), None);
    let mut handler = RecordingHandler::default();

    run_report_cycles(
        &config,
        &test_basic_info(),
        &FixedReportGenerator(test_report(31.0)),
        &mut http,
        &mut websocket,
        &mut handler,
        2,
    )
    .unwrap();

    assert_eq!(
        events.borrow().as_slice(),
        [
            "upload:https://panel.example.com/api/clients/uploadBasicInfo?token=secret-token-value",
            "connect:wss://panel.example.com/api/clients/report?token=secret-token-value",
            "send_report",
            "send_ping",
            "upload:https://panel.example.com/api/clients/uploadBasicInfo?token=secret-token-value",
            "upload_error",
            "upload:https://panel.example.com/api/clients/uploadBasicInfo?token=secret-token-value",
            "upload_error",
            "send_report",
            "send_ping",
        ]
    );
}

#[test]
fn run_report_cycles_with_delay_waits_between_cycles() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut config = test_config();
    config.interval_seconds = 2.0;
    let mut http = FakeHttp::new(events.clone());
    let mut websocket = FakeWebSocketTransport::new(events.clone(), None);
    let mut handler = RecordingHandler::default();
    let mut delay = RecordingDelay::new(events.clone());

    run_report_cycles_with_delay(
        &config,
        &test_basic_info(),
        &FixedReportGenerator(test_report(11.0)),
        &mut http,
        &mut websocket,
        &mut handler,
        &mut delay,
        2,
    )
    .unwrap();

    assert_eq!(
        events.borrow().as_slice(),
        [
            "upload:https://panel.example.com/api/clients/uploadBasicInfo?token=secret-token-value",
            "connect:wss://panel.example.com/api/clients/report?token=secret-token-value",
            "send_report",
            "sleep_report:1",
            "send_report"
        ]
    );
}

#[test]
fn run_report_cycles_drains_available_control_messages_after_report() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let inbound_messages = vec![
        br#"{"message":"terminal","request_id":"terminal-1"}"#.to_vec(),
        br#"{"message":"exec","task_id":"task-1","command":"uptime"}"#.to_vec(),
    ];
    let mut http = FakeHttp::new(events.clone());
    let mut websocket =
        FakeWebSocketTransport::new(events.clone(), None).with_inbound_messages(inbound_messages);
    let mut handler = RecordingHandler::default();

    run_report_cycles(
        &test_config(),
        &test_basic_info(),
        &FixedReportGenerator(test_report(11.0)),
        &mut http,
        &mut websocket,
        &mut handler,
        1,
    )
    .unwrap();

    assert_eq!(
        events.borrow().as_slice(),
        [
            "upload:https://panel.example.com/api/clients/uploadBasicInfo?token=secret-token-value",
            "connect:wss://panel.example.com/api/clients/report?token=secret-token-value",
            "send_report"
        ]
    );
    assert_eq!(
        handler.messages,
        vec![
            BackendMessage::Terminal {
                request_id: "terminal-1".to_string(),
            },
            BackendMessage::Exec {
                task_id: "task-1".to_string(),
                command: "uptime".to_string(),
            },
        ]
    );
}

#[test]
fn run_report_cycles_executes_ping_message_and_sends_result() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let sent_ping_results = Rc::new(RefCell::new(Vec::new()));
    let inbound =
        br#"{"message":"ping","ping_task_id":7,"ping_type":"tcp","ping_target":"1.1.1.1:443"}"#
            .to_vec();
    let mut http = FakeHttp::new(events.clone());
    let mut websocket = FakeWebSocketTransport::new(events.clone(), Some(inbound))
        .with_sent_ping_results(sent_ping_results.clone());
    let mut handler = RecordingHandler::default();
    let mut delay = RecordingDelay::new(events.clone());

    run_report_cycles_with_ping_delay(
        &test_config(),
        &test_basic_info(),
        &FixedReportGenerator(test_report(11.0)),
        &FixedPingExecutor::new(25),
        &mut http,
        &mut websocket,
        &mut handler,
        &mut delay,
        1,
    )
    .unwrap();

    assert_eq!(
        events.borrow().as_slice(),
        [
            "upload:https://panel.example.com/api/clients/uploadBasicInfo?token=secret-token-value",
            "connect:wss://panel.example.com/api/clients/report?token=secret-token-value",
            "send_report",
            "send_ping_result"
        ]
    );
    assert_eq!(sent_ping_results.borrow()[0].task_id, 7);
    assert_eq!(sent_ping_results.borrow()[0].ping_type, "tcp");
    assert_eq!(sent_ping_results.borrow()[0].value, 25);
    assert!(handler.messages.is_empty());
}

fn test_config() -> AgentConfig {
    AgentConfig {
        endpoint: "https://panel.example.com".to_string(),
        token: "secret-token-value".to_string(),
        insecure: true,
        disable_web_ssh: true,
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

fn test_basic_info() -> BasicInfo {
    BasicInfo {
        cpu_name: "CPU".to_string(),
        cpu_cores: 2,
        arch: "x86_64".to_string(),
        os: "linux".to_string(),
        kernel_version: "6.8".to_string(),
        ipv4: "127.0.0.1".to_string(),
        ipv6: "::1".to_string(),
        mem_total: 1024,
        swap_total: 0,
        disk_total: 4096,
        gpu_name: String::new(),
        virtualization: "kvm".to_string(),
        version: "rs-test".to_string(),
    }
}

fn test_report(cpu_usage: f64) -> Report {
    Report {
        cpu: CpuReport { usage: cpu_usage },
        ram: MemoryReport {
            total: 1024,
            used: 512,
        },
        swap: MemoryReport { total: 0, used: 0 },
        load: LoadReport {
            load1: 0.1,
            load5: 0.1,
            load15: 0.1,
        },
        disk: DiskReport {
            total: 4096,
            used: 1024,
        },
        network: NetworkReport {
            up: 1,
            down: 2,
            total_up: 3,
            total_down: 4,
        },
        connections: ConnectionsReport { tcp: 5, udp: 6 },
        uptime: 7,
        process: 8,
        gpu: None,
        cn_connectivity: None,
        message: String::new(),
    }
}

struct FixedReportGenerator(Report);

impl ReportGenerator for FixedReportGenerator {
    fn generate(&self) -> Report {
        self.0.clone()
    }
}

struct RotatingBasicInfoProvider {
    kernel_versions: Vec<String>,
    index: RefCell<usize>,
}

impl RotatingBasicInfoProvider {
    fn new<const N: usize>(kernel_versions: [&str; N]) -> Self {
        Self {
            kernel_versions: kernel_versions
                .into_iter()
                .map(ToString::to_string)
                .collect(),
            index: RefCell::new(0),
        }
    }

    fn basic_info(&self) -> BasicInfo {
        let mut basic_info = test_basic_info();
        let mut index = self.index.borrow_mut();
        let kernel_version = self
            .kernel_versions
            .get(*index)
            .cloned()
            .unwrap_or_else(|| self.kernel_versions.last().cloned().unwrap_or_default());
        *index += 1;
        basic_info.kernel_version = kernel_version;
        basic_info
    }
}

struct FakeHttp {
    events: Rc<RefCell<Vec<String>>>,
    upload_failures_remaining: Rc<RefCell<usize>>,
    upload_failures_after_successes: Rc<RefCell<Option<(usize, usize)>>>,
    uploaded_kernel_versions: Rc<RefCell<Vec<String>>>,
}

impl FakeHttp {
    fn new(events: Rc<RefCell<Vec<String>>>) -> Self {
        Self {
            events,
            upload_failures_remaining: Rc::new(RefCell::new(0)),
            upload_failures_after_successes: Rc::new(RefCell::new(None)),
            uploaded_kernel_versions: Rc::new(RefCell::new(Vec::new())),
        }
    }

    fn with_upload_failures(self, failures: usize) -> Self {
        *self.upload_failures_remaining.borrow_mut() = failures;
        self
    }

    fn with_upload_failures_after_successes(self, successes: usize, failures: usize) -> Self {
        *self.upload_failures_after_successes.borrow_mut() = Some((successes, failures));
        self
    }

    fn with_uploaded_kernel_versions(
        mut self,
        uploaded_kernel_versions: Rc<RefCell<Vec<String>>>,
    ) -> Self {
        self.uploaded_kernel_versions = uploaded_kernel_versions;
        self
    }
}

impl HttpTransport for FakeHttp {
    fn upload_basic_info(
        &mut self,
        url: &str,
        _headers: &[HeaderPair],
        basic_info: &BasicInfo,
    ) -> Result<(), TransportError> {
        self.events.borrow_mut().push(format!("upload:{url}"));
        self.uploaded_kernel_versions
            .borrow_mut()
            .push(basic_info.kernel_version.clone());
        let mut failures = self.upload_failures_remaining.borrow_mut();
        if *failures > 0 {
            *failures -= 1;
            self.events.borrow_mut().push("upload_error".to_string());
            return Err(TransportError::RequestFailed("legacy schema".to_string()));
        }
        drop(failures);

        let mut delayed_failures = self.upload_failures_after_successes.borrow_mut();
        if let Some((successes_before_failure, failures_remaining)) = delayed_failures.as_mut() {
            if *successes_before_failure == 0 && *failures_remaining > 0 {
                *failures_remaining -= 1;
                self.events.borrow_mut().push("upload_error".to_string());
                if *failures_remaining == 0 {
                    *delayed_failures = None;
                }
                return Err(TransportError::RequestFailed(
                    "periodic upload failed".to_string(),
                ));
            }
            *successes_before_failure = successes_before_failure.saturating_sub(1);
        }
        Ok(())
    }
}

struct FakeWebSocketTransport {
    events: Rc<RefCell<Vec<String>>>,
    inbound: VecDeque<Vec<u8>>,
    sent_reports: Rc<RefCell<Vec<Report>>>,
    sent_ping_results: Rc<RefCell<Vec<PingResult>>>,
    send_failures_remaining: Rc<RefCell<usize>>,
}

impl FakeWebSocketTransport {
    fn new(events: Rc<RefCell<Vec<String>>>, inbound: Option<Vec<u8>>) -> Self {
        Self {
            events,
            inbound: inbound.into_iter().collect(),
            sent_reports: Rc::new(RefCell::new(Vec::new())),
            sent_ping_results: Rc::new(RefCell::new(Vec::new())),
            send_failures_remaining: Rc::new(RefCell::new(0)),
        }
    }

    fn with_inbound_messages<I>(mut self, messages: I) -> Self
    where
        I: IntoIterator<Item = Vec<u8>>,
    {
        self.inbound = messages.into_iter().collect();
        self
    }

    fn with_sent_reports(mut self, sent_reports: Rc<RefCell<Vec<Report>>>) -> Self {
        self.sent_reports = sent_reports;
        self
    }

    fn with_send_failures(self, failures: usize) -> Self {
        *self.send_failures_remaining.borrow_mut() = failures;
        self
    }

    fn with_sent_ping_results(mut self, sent_ping_results: Rc<RefCell<Vec<PingResult>>>) -> Self {
        self.sent_ping_results = sent_ping_results;
        self
    }
}

impl WebSocketTransport for FakeWebSocketTransport {
    type Socket = FakeSocket;

    fn connect_report(
        &mut self,
        url: &str,
        _headers: &[HeaderPair],
    ) -> Result<Self::Socket, TransportError> {
        self.events.borrow_mut().push(format!("connect:{url}"));
        Ok(FakeSocket {
            events: self.events.clone(),
            inbound: std::mem::take(&mut self.inbound),
            sent_reports: self.sent_reports.clone(),
            sent_ping_results: self.sent_ping_results.clone(),
            send_failures_remaining: self.send_failures_remaining.clone(),
        })
    }
}

struct FakeSocket {
    events: Rc<RefCell<Vec<String>>>,
    inbound: VecDeque<Vec<u8>>,
    sent_reports: Rc<RefCell<Vec<Report>>>,
    sent_ping_results: Rc<RefCell<Vec<PingResult>>>,
    send_failures_remaining: Rc<RefCell<usize>>,
}

impl ReportSocket for FakeSocket {
    fn send_report(&mut self, report: &Report) -> Result<(), TransportError> {
        let mut failures = self.send_failures_remaining.borrow_mut();
        if *failures > 0 {
            *failures -= 1;
            self.events
                .borrow_mut()
                .push("send_report_error".to_string());
            return Err(TransportError::RequestFailed("send failed".to_string()));
        }
        drop(failures);

        self.events.borrow_mut().push("send_report".to_string());
        self.sent_reports.borrow_mut().push(report.clone());
        Ok(())
    }

    fn read_message(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        Ok(self.inbound.pop_front())
    }

    fn send_ping(&mut self) -> Result<(), TransportError> {
        self.events.borrow_mut().push("send_ping".to_string());
        Ok(())
    }

    fn send_ping_result(&mut self, result: &PingResult) -> Result<(), TransportError> {
        self.events
            .borrow_mut()
            .push("send_ping_result".to_string());
        self.sent_ping_results.borrow_mut().push(result.clone());
        Ok(())
    }
}

#[derive(Default)]
struct RecordingHandler {
    messages: Vec<BackendMessage>,
}

impl ControlMessageHandler for RecordingHandler {
    fn handle(&mut self, message: BackendMessage) {
        self.messages.push(message);
    }
}

struct RecordingDelay {
    events: Rc<RefCell<Vec<String>>>,
}

impl RecordingDelay {
    fn new(events: Rc<RefCell<Vec<String>>>) -> Self {
        Self { events }
    }
}

impl LoopDelay for RecordingDelay {
    fn sleep_report_interval(&mut self, seconds: f64) {
        self.events
            .borrow_mut()
            .push(format!("sleep_report:{seconds}"));
    }

    fn sleep_reconnect_interval(&mut self, seconds: u64) {
        self.events
            .borrow_mut()
            .push(format!("sleep_reconnect:{seconds}"));
    }
}
