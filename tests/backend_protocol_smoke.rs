use kelicloud_agent_rs::backend_protocol_smoke::{sample_basic_info, StaticSmokeReportGenerator};
use kelicloud_agent_rs::report::ReportGenerator;

#[test]
fn sample_basic_info_uses_linux_compatible_identity() {
    let info = sample_basic_info("0.1.0-smoke");

    assert_eq!(info.os, "linux");
    assert_eq!(info.arch, "amd64");
    assert_eq!(info.version, "0.1.0-smoke");
    assert!(info.cpu_cores >= 1);
    assert!(info.mem_total > 0);
    assert!(info.disk_total > 0);
}

#[test]
fn sample_report_contains_backend_accepted_metrics() {
    let report = StaticSmokeReportGenerator.generate();

    assert!(report.cpu.usage > 0.0);
    assert!(report.cpu.usage <= 100.0);
    assert!(report.ram.total >= report.ram.used);
    assert!(report.disk.total >= report.disk.used);
    assert!(report.connections.tcp >= 0);
    assert!(report.connections.udp >= 0);
    assert_eq!(report.message, "backend-protocol-smoke");
}
