use kelicloud_agent_rs::config::AgentConfig;
use kelicloud_agent_rs::linux_proc::{GpuMetric, IpAddresses, ProcMetricErrors};
use kelicloud_agent_rs::report::{GpuReport, ReportGenerator};
use kelicloud_agent_rs::system::{
    append_report_error, go_compatible_cpu_usage, gpu_report_from_detailed_result,
    gpu_report_from_metrics, proc_metric_errors_to_message, select_basic_info_ip_addresses,
    SystemMetricsOptions, SystemReportGenerator, SystemSnapshot, SystemSnapshotCollector,
};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn system_snapshot_maps_to_report_and_clamps_cpu_usage() {
    let snapshot = SystemSnapshot {
        cpu_name: "AMD EPYC".to_string(),
        cpu_cores: 4,
        arch: "x86_64".to_string(),
        os: "linux".to_string(),
        kernel_version: "6.8.0".to_string(),
        ipv4: "10.0.0.5".to_string(),
        ipv6: "2607:f358:1a:e::ab0:39b7".to_string(),
        mem_total: 8192,
        mem_used: 4096,
        swap_total: 2048,
        swap_used: 1024,
        disk_total: 100_000,
        disk_used: 40_000,
        load1: 0.1,
        load5: 0.2,
        load15: 0.3,
        network_up: 700,
        network_down: 500,
        network_total_up: 2700,
        network_total_down: 1500,
        tcp_connections: 12,
        udp_connections: 3,
        uptime: 3600,
        process_count: 88,
        cpu_usage: 0.0,
        virtualization: "kvm".to_string(),
        gpu_name: "NVIDIA Corporation GA102 [GeForce RTX 3090]".to_string(),
        message: "failed to get network speed: permission denied\n".to_string(),
        gpu_report: Some(GpuReport {
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
    };

    let report = snapshot.to_report();

    assert_eq!(report.cpu.usage, 0.001);
    assert_eq!(report.ram.total, 8192);
    assert_eq!(report.ram.used, 4096);
    assert_eq!(report.swap.total, 2048);
    assert_eq!(report.swap.used, 1024);
    assert_eq!(report.disk.total, 100_000);
    assert_eq!(report.disk.used, 40_000);
    assert_eq!(report.load.load1, 0.1);
    assert_eq!(report.load.load5, 0.2);
    assert_eq!(report.load.load15, 0.3);
    assert_eq!(report.network.up, 700);
    assert_eq!(report.network.down, 500);
    assert_eq!(report.network.total_up, 2700);
    assert_eq!(report.network.total_down, 1500);
    assert_eq!(report.uptime, 3600);
    assert_eq!(report.process, 88);
    assert_eq!(report.connections.tcp, 12);
    assert_eq!(report.connections.udp, 3);
    assert_eq!(
        report.message,
        "failed to get network speed: permission denied\n"
    );
    assert_eq!(
        report.gpu.unwrap().models.unwrap()[0],
        "NVIDIA Corporation GA102 [GeForce RTX 3090]"
    );
    let cn_connectivity = report.cn_connectivity.unwrap();
    assert_eq!(cn_connectivity["status"], "unknown");
    assert_eq!(cn_connectivity["target"], "223.5.5.5");
    assert_eq!(cn_connectivity["message"], "waiting for probe");
}

#[test]
fn go_compatible_cpu_usage_matches_go_agent_floor() {
    assert_eq!(go_compatible_cpu_usage(0.0), 0.001);
    assert_eq!(go_compatible_cpu_usage(0.0005), 0.001);
    assert_eq!(go_compatible_cpu_usage(0.001), 0.001);
    assert_eq!(go_compatible_cpu_usage(0.005), 0.005);
}

#[test]
fn select_basic_info_ip_addresses_matches_go_agent_nic_and_custom_priority() {
    let nic_addresses = IpAddresses {
        ipv4: "10.0.0.5".to_string(),
        ipv6: String::new(),
    };
    let public_addresses = IpAddresses {
        ipv4: "198.51.100.10".to_string(),
        ipv6: "2607:f358:1a:e::ab0:39b7".to_string(),
    };

    assert_eq!(
        select_basic_info_ip_addresses(
            true,
            Some(nic_addresses.clone()),
            public_addresses.clone(),
            "203.0.113.10",
            "2001:db8::10",
        ),
        nic_addresses
    );

    assert_eq!(
        select_basic_info_ip_addresses(
            true,
            Some(IpAddresses::default()),
            public_addresses.clone(),
            "203.0.113.10",
            "",
        ),
        IpAddresses {
            ipv4: "203.0.113.10".to_string(),
            ipv6: "2607:f358:1a:e::ab0:39b7".to_string(),
        }
    );
}

#[test]
fn system_snapshot_maps_to_basic_info() {
    let snapshot = SystemSnapshot {
        cpu_name: "AMD EPYC".to_string(),
        cpu_cores: 4,
        arch: "x86_64".to_string(),
        os: "linux".to_string(),
        kernel_version: "6.8.0".to_string(),
        ipv4: "10.0.0.5".to_string(),
        ipv6: "2607:f358:1a:e::ab0:39b7".to_string(),
        mem_total: 8192,
        mem_used: 4096,
        swap_total: 2048,
        swap_used: 1024,
        disk_total: 100_000,
        disk_used: 40_000,
        load1: 0.1,
        load5: 0.2,
        load15: 0.3,
        network_up: 700,
        network_down: 500,
        network_total_up: 2700,
        network_total_down: 1500,
        tcp_connections: 12,
        udp_connections: 3,
        uptime: 3600,
        process_count: 88,
        cpu_usage: 12.5,
        virtualization: "kvm".to_string(),
        gpu_name: "NVIDIA Corporation GA102 [GeForce RTX 3090]".to_string(),
        message: String::new(),
        gpu_report: None,
        cn_connectivity: None,
    };

    let basic_info = snapshot.to_basic_info("rs-test");

    assert_eq!(basic_info.cpu_name, "AMD EPYC");
    assert_eq!(basic_info.cpu_cores, 4);
    assert_eq!(basic_info.arch, "x86_64");
    assert_eq!(basic_info.os, "linux");
    assert_eq!(basic_info.kernel_version, "6.8.0");
    assert_eq!(basic_info.ipv4, "10.0.0.5");
    assert_eq!(basic_info.ipv6, "2607:f358:1a:e::ab0:39b7");
    assert_eq!(basic_info.mem_total, 8192);
    assert_eq!(basic_info.swap_total, 2048);
    assert_eq!(basic_info.disk_total, 100_000);
    assert_eq!(
        basic_info.gpu_name,
        "NVIDIA Corporation GA102 [GeForce RTX 3090]"
    );
    assert_eq!(basic_info.virtualization, "kvm");
    assert_eq!(basic_info.version, "rs-test");
}

#[test]
fn system_collector_returns_platform_snapshot() {
    let snapshot = SystemSnapshotCollector::new().collect();

    assert!(snapshot.cpu_cores >= 1);
    assert!(!snapshot.arch.is_empty());
    assert!(!snapshot.os.is_empty());
    assert!(!snapshot.virtualization.is_empty());
    assert!(snapshot.mem_total >= snapshot.mem_used);
    assert!(snapshot.disk_total >= snapshot.disk_used);
    if kelicloud_agent_rs::linux_proc::linux_supported() {
        assert!(snapshot.uptime > 0);
    }
}

#[test]
fn system_metrics_options_follow_agent_config() {
    let config = AgentConfig::from_args_and_env(
        [
            "kelicloud-agent-rs",
            "--endpoint",
            "https://panel.example.com",
            "--token",
            "token",
            "--include-nics",
            "eth0",
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
        ],
        |key| match key {
            "HOST_PROC" => Some("/host/proc".to_string()),
            _ => None,
        },
    )
    .unwrap();

    let options = SystemMetricsOptions::from(&config);

    assert_eq!(options.include_nics, "eth0");
    assert_eq!(options.exclude_nics, "docker0");
    assert_eq!(options.include_mountpoints, "/;/data");
    assert_eq!(options.custom_ipv4, "203.0.113.10");
    assert_eq!(options.custom_ipv6, "2607:f358:1a:e::ab0:39b7");
    assert!(options.get_ip_addr_from_nic);
    assert!(options.memory_include_cache);
    assert!(options.memory_report_raw_used);
    assert!(options.enable_gpu);
    assert!(options.public_ip_probe);
    assert_eq!(options.month_rotate, 15);
    assert_eq!(options.host_proc, "/host/proc");
    assert_eq!(options.network_speed_sample_millis, 1_000);
    assert!(!SystemMetricsOptions::default().public_ip_probe);
    assert_eq!(
        SystemMetricsOptions::default().network_speed_sample_millis,
        1_000
    );
}

#[test]
fn collector_uses_net_static_month_rotate_totals_when_available() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let path = std::env::temp_dir().join(format!(
        "kelicloud-agent-rs-net-static-{}-{}.json",
        std::process::id(),
        now
    ));
    fs::write(
        &path,
        format!(
            r#"{{
  "interfaces": {{
    "eth-test": [{{"timestamp": {now}, "tx": 123, "rx": 456}}],
    "docker0": [{{"timestamp": {now}, "tx": 999, "rx": 999}}]
  }}
}}"#
        ),
    )
    .unwrap();

    let mut collector = SystemSnapshotCollector::with_metrics(SystemMetricsOptions {
        include_nics: "eth-test".to_string(),
        month_rotate: 1,
        net_static_path: path.to_string_lossy().into_owned(),
        ..SystemMetricsOptions::default()
    });

    let snapshot = collector.collect();
    drop(collector);
    let _ = fs::remove_file(path);

    assert_eq!(snapshot.network_total_up, 123);
    assert_eq!(snapshot.network_total_down, 456);
}

#[test]
fn gpu_report_from_metrics_matches_go_agent_detailed_shape() {
    let report = gpu_report_from_metrics(
        "NVIDIA GeForce RTX 3090",
        vec![
            GpuMetric {
                name: "NVIDIA GeForce RTX 3090".to_string(),
                memory_total: 24_576 * 1024 * 1024,
                memory_used: 1024 * 1024 * 1024,
                utilization: 25.0,
                temperature: 65,
            },
            GpuMetric {
                name: "NVIDIA GeForce RTX 4090".to_string(),
                memory_total: 24_576 * 1024 * 1024,
                memory_used: 2048 * 1024 * 1024,
                utilization: 35.0,
                temperature: 70,
            },
        ],
    )
    .unwrap();

    assert_eq!(report.count, Some(2));
    assert_eq!(report.average_usage, Some(30.0));
    assert_eq!(
        report.detailed_info.as_ref().unwrap()[0].name,
        "NVIDIA GeForce RTX 3090"
    );
    assert!(report.models.is_none());
}

#[test]
fn append_report_error_matches_go_agent_message_lines() {
    let mut message = String::new();

    append_report_error(&mut message, "network speed", "no network interfaces found");
    append_report_error(&mut message, "uptime", "permission denied");

    assert_eq!(
        message,
        "failed to get network speed: no network interfaces found\nfailed to get uptime: permission denied\n"
    );
}

#[test]
fn proc_metric_errors_to_message_matches_go_agent_report_labels() {
    let message = proc_metric_errors_to_message(&ProcMetricErrors {
        network_speed: Some(
            "failed to get network IO counters: /proc/net/dev is not readable".to_string(),
        ),
        connections: Some(
            "failed to get TCP connections: /proc/net/tcp is not readable".to_string(),
        ),
        uptime: Some("/proc/uptime is not readable or invalid".to_string()),
    });

    assert_eq!(
        message,
        "failed to get network speed: failed to get network IO counters: /proc/net/dev is not readable\nfailed to get connections: failed to get TCP connections: /proc/net/tcp is not readable\nfailed to get uptime: /proc/uptime is not readable or invalid\n"
    );
}

#[test]
fn gpu_report_from_detailed_result_falls_back_to_models_and_message_on_error() {
    let (report, message) = gpu_report_from_detailed_result(
        "NVIDIA GeForce RTX 3090",
        Err("nvidia-smi not found".to_string()),
    );

    assert_eq!(
        message,
        "failed to get detailed GPU info: nvidia-smi not found\n"
    );
    assert_eq!(
        report.unwrap().models.unwrap(),
        vec!["NVIDIA GeForce RTX 3090".to_string()]
    );
}

#[test]
fn gpu_report_from_detailed_result_keeps_detailed_info_without_message() {
    let (report, message) = gpu_report_from_detailed_result(
        "NVIDIA GeForce RTX 3090",
        Ok(vec![GpuMetric {
            name: "NVIDIA GeForce RTX 3090".to_string(),
            memory_total: 24_576 * 1024 * 1024,
            memory_used: 1024 * 1024 * 1024,
            utilization: 25.0,
            temperature: 65,
        }]),
    );

    assert!(message.is_empty());
    assert_eq!(report.unwrap().count, Some(1));
}

#[test]
fn system_report_generator_produces_backend_report() {
    let generator = SystemReportGenerator::new(SystemSnapshotCollector::new());
    let report = generator.generate();

    assert!(report.cpu.usage >= 0.001);
    assert!(report.ram.total >= report.ram.used);
    assert!(report.disk.total >= report.disk.used);
}
