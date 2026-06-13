use kelicloud_agent_rs::report::{
    go_runtime_arch_name, BasicInfo, ConnectionsReport, CpuReport, DiskReport, GpuReport,
    LoadReport, MemoryReport, NetworkReport, Report, ReportGenerator, StaticReportGenerator,
};

#[test]
fn report_serializes_backend_common_report_shape() {
    let report = Report {
        cpu: CpuReport { usage: 12.5 },
        ram: MemoryReport {
            total: 2048,
            used: 1024,
        },
        swap: MemoryReport {
            total: 1024,
            used: 128,
        },
        load: LoadReport {
            load1: 0.5,
            load5: 0.4,
            load15: 0.3,
        },
        disk: DiskReport {
            total: 4096,
            used: 1024,
        },
        network: NetworkReport {
            up: 128,
            down: 256,
            total_up: 1024,
            total_down: 2048,
        },
        connections: ConnectionsReport { tcp: 10, udp: 2 },
        uptime: 3600,
        process: 42,
        gpu: None,
        cn_connectivity: None,
        message: String::new(),
    };

    let value = serde_json::to_value(report).unwrap();

    assert_eq!(value["cpu"]["usage"], 12.5);
    assert_eq!(value["ram"]["total"], 2048);
    assert_eq!(value["network"]["totalUp"], 1024);
    assert_eq!(value["network"]["totalDown"], 2048);
    assert_eq!(value["connections"]["tcp"], 10);
    assert_eq!(value["uptime"], 3600);
    assert_eq!(value["message"], "");
}

#[test]
fn report_serializes_optional_gpu_and_cn_connectivity_shape() {
    let report = Report {
        cpu: CpuReport { usage: 12.5 },
        ram: MemoryReport {
            total: 2048,
            used: 1024,
        },
        swap: MemoryReport {
            total: 1024,
            used: 128,
        },
        load: LoadReport {
            load1: 0.5,
            load5: 0.4,
            load15: 0.3,
        },
        disk: DiskReport {
            total: 4096,
            used: 1024,
        },
        network: NetworkReport {
            up: 128,
            down: 256,
            total_up: 1024,
            total_down: 2048,
        },
        connections: ConnectionsReport { tcp: 10, udp: 2 },
        uptime: 3600,
        process: 42,
        gpu: Some(GpuReport {
            models: Some(vec![
                "NVIDIA Corporation GA102 [GeForce RTX 3090]".to_string()
            ]),
            count: None,
            average_usage: None,
            detailed_info: None,
        }),
        cn_connectivity: Some(serde_json::json!({
            "status": "unknown",
            "target": "223.5.5.5",
            "message": "waiting for probe"
        })),
        message: String::new(),
    };

    let value = serde_json::to_value(report).unwrap();

    assert_eq!(
        value["gpu"]["models"][0],
        "NVIDIA Corporation GA102 [GeForce RTX 3090]"
    );
    assert_eq!(value["cn_connectivity"]["status"], "unknown");
    assert_eq!(value["cn_connectivity"]["target"], "223.5.5.5");
    assert_eq!(value["cn_connectivity"]["message"], "waiting for probe");
}

#[test]
fn basic_info_serializes_upload_basic_info_shape() {
    let basic_info = BasicInfo {
        cpu_name: "AMD EPYC".to_string(),
        cpu_cores: 4,
        arch: "x86_64".to_string(),
        os: "linux".to_string(),
        kernel_version: "6.8.0".to_string(),
        ipv4: "203.0.113.10".to_string(),
        ipv6: "::1".to_string(),
        mem_total: 8192,
        swap_total: 1024,
        disk_total: 100_000,
        gpu_name: "NVIDIA Corporation GA102 [GeForce RTX 3090]".to_string(),
        virtualization: "kvm".to_string(),
        version: "rs-0.1.0".to_string(),
    };

    let value = serde_json::to_value(basic_info).unwrap();

    assert_eq!(value["cpu_name"], "AMD EPYC");
    assert_eq!(value["cpu_cores"], 4);
    assert_eq!(value["kernel_version"], "6.8.0");
    assert_eq!(value["mem_total"], 8192);
    assert_eq!(
        value["gpu_name"],
        "NVIDIA Corporation GA102 [GeForce RTX 3090]"
    );
    assert_eq!(value["version"], "rs-0.1.0");
}

#[test]
fn basic_info_without_kernel_version_serializes_legacy_upload_shape() {
    let basic_info = BasicInfo {
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
        version: "rs-0.1.0".to_string(),
    };

    let value = serde_json::to_value(basic_info.without_kernel_version()).unwrap();

    assert!(value.get("kernel_version").is_none());
    assert_eq!(value["cpu_name"], "AMD EPYC");
    assert_eq!(value["version"], "rs-0.1.0");
}

#[test]
fn minimal_basic_info_contains_platform_and_version() {
    let basic_info = BasicInfo::minimal("rs-test");

    assert!(!basic_info.arch.is_empty());
    assert!(!basic_info.os.is_empty());
    assert_eq!(basic_info.version, "rs-test");
    assert!(basic_info.cpu_cores >= 1);
}

#[test]
fn go_runtime_arch_name_matches_go_basic_info_arch_values() {
    assert_eq!(go_runtime_arch_name("x86_64"), "amd64");
    assert_eq!(go_runtime_arch_name("aarch64"), "arm64");
    assert_eq!(go_runtime_arch_name("x86"), "386");
    assert_eq!(go_runtime_arch_name("riscv64"), "riscv64");
}

#[test]
fn static_report_generator_returns_valid_placeholder_report() {
    let generator = StaticReportGenerator::default();
    let report = generator.generate();

    assert!(report.cpu.usage >= 0.001);
    assert_eq!(report.connections.tcp, 0);
    assert_eq!(report.connections.udp, 0);
    assert_eq!(report.message, "");
}
