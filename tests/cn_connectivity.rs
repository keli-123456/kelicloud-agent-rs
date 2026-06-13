use kelicloud_agent_rs::cn_connectivity::{
    probe_target_with_retries, CnConnectivityControlMessageHandler, CnConnectivityReportGenerator,
    CnConnectivityState,
};
use kelicloud_agent_rs::protocol::BackendMessage;
use kelicloud_agent_rs::report::{
    ConnectionsReport, CpuReport, DiskReport, LoadReport, MemoryReport, NetworkReport, Report,
    ReportGenerator,
};
use kelicloud_agent_rs::runtime::ControlMessageHandler;

#[test]
fn cn_connectivity_config_creates_go_compatible_waiting_result() {
    let state = CnConnectivityState::default();
    let mut handler = CnConnectivityControlMessageHandler::new(state.clone());

    handler.handle(BackendMessage::CnConnectivityProbeConfig {
        enabled: true,
        target: Some(" 223.5.5.5\r\n119.29.29.29,223.5.5.5; ".to_string()),
        interval_seconds: 0,
        retry_attempts: 0,
        retry_delay_seconds: 0,
        timeout_seconds: 0,
    });

    let value = state.current_report_value().unwrap();
    assert_eq!(value["status"], "unknown");
    assert_eq!(value["target"], "223.5.5.5");
    assert_eq!(value["message"], "waiting for probe");
    assert!(value.get("latency").is_none());
    assert!(value.get("checked_at").is_none());
    assert!(value.get("consecutive_failures").is_none());
}

#[test]
fn cn_connectivity_disabled_or_empty_targets_clears_report_value() {
    let state = CnConnectivityState::default();
    let mut handler = CnConnectivityControlMessageHandler::new(state.clone());

    handler.handle(BackendMessage::CnConnectivityProbeConfig {
        enabled: true,
        target: Some("223.5.5.5".to_string()),
        interval_seconds: 60,
        retry_attempts: 3,
        retry_delay_seconds: 1,
        timeout_seconds: 5,
    });
    assert!(state.current_report_value().is_some());

    handler.handle(BackendMessage::CnConnectivityProbeConfig {
        enabled: false,
        target: Some("223.5.5.5".to_string()),
        interval_seconds: 60,
        retry_attempts: 3,
        retry_delay_seconds: 1,
        timeout_seconds: 5,
    });
    assert!(state.current_report_value().is_none());

    handler.handle(BackendMessage::CnConnectivityProbeConfig {
        enabled: true,
        target: Some(" , ; \n ".to_string()),
        interval_seconds: 60,
        retry_attempts: 3,
        retry_delay_seconds: 1,
        timeout_seconds: 5,
    });
    assert!(state.current_report_value().is_none());
}

#[test]
fn cn_connectivity_report_generator_injects_current_state() {
    let state = CnConnectivityState::default();
    let mut handler = CnConnectivityControlMessageHandler::new(state.clone());
    handler.handle(BackendMessage::CnConnectivityProbeConfig {
        enabled: true,
        target: Some("223.5.5.5\n119.29.29.29".to_string()),
        interval_seconds: 60,
        retry_attempts: 3,
        retry_delay_seconds: 1,
        timeout_seconds: 5,
    });

    let generator = CnConnectivityReportGenerator::new(FixedReportGenerator, state);
    let report = generator.generate();

    assert_eq!(report.cn_connectivity.unwrap()["target"], "223.5.5.5");
}

#[test]
fn cn_connectivity_probe_retries_until_success() {
    let mut attempts = 0;
    let mut wait_calls = 0;

    let result = probe_target_with_retries(
        "223.5.5.5",
        3,
        |target| {
            attempts += 1;
            assert_eq!(target, "223.5.5.5");
            if attempts < 3 {
                Err("no packets received".to_string())
            } else {
                Ok(42)
            }
        },
        || {
            wait_calls += 1;
        },
    );

    assert_eq!(attempts, 3);
    assert_eq!(wait_calls, 2);
    assert_eq!(result.status, "ok");
    assert_eq!(result.target, "223.5.5.5");
    assert_eq!(result.latency, Some(42));
    assert_eq!(result.message, "icmp reachable");
    assert!(result.checked_at.is_some());
}

#[test]
fn cn_connectivity_state_marks_second_consecutive_failure_blocked_suspected() {
    let state = CnConnectivityState::default();
    let mut handler = CnConnectivityControlMessageHandler::new(state.clone());
    handler.handle(BackendMessage::CnConnectivityProbeConfig {
        enabled: true,
        target: Some("223.5.5.5".to_string()),
        interval_seconds: 60,
        retry_attempts: 1,
        retry_delay_seconds: 1,
        timeout_seconds: 5,
    });

    state.probe_once_with(
        |_| Err("no packets received".to_string()),
        || panic!("single-attempt probe should not wait before retry"),
    );
    let first = state.current_report_value().unwrap();
    assert_eq!(first["status"], "degraded");
    assert_eq!(first["consecutive_failures"], 1);

    state.probe_once_with(
        |_| Err("no packets received".to_string()),
        || panic!("single-attempt probe should not wait before retry"),
    );
    let second = state.current_report_value().unwrap();
    assert_eq!(second["status"], "blocked_suspected");
    assert_eq!(second["consecutive_failures"], 2);

    state.probe_once_with(|_| Ok(21), || {});
    let recovered = state.current_report_value().unwrap();
    assert_eq!(recovered["status"], "ok");
    assert!(recovered.get("consecutive_failures").is_none());
}

struct FixedReportGenerator;

impl ReportGenerator for FixedReportGenerator {
    fn generate(&self) -> Report {
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
            process: 0,
            gpu: None,
            cn_connectivity: None,
            message: String::new(),
        }
    }
}
