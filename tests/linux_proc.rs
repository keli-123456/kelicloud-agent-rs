use chrono::{Local, TimeZone};
use kelicloud_agent_rs::linux_proc::{
    android_os_name_from_build_prop, collect_proc_metrics_sample_with_filter_and_proc_root,
    count_process_entries, count_process_entries_in_dir, count_socket_entries,
    cpu_name_from_sources, cpu_usage_percent_from_proc_stat_samples, detect_container_from_cgroup,
    detect_container_from_markers, fnos_os_name_from_markers, go_compatible_disk,
    go_compatible_ram, go_compatible_ram_include_cache, go_compatible_ram_raw_used,
    go_compatible_swap, kernel_version_from_uname_output, linux_supported,
    memory_selection_from_meminfo_with_modes, memory_values_from_meminfo_with_modes,
    network_speed_from_samples, nic_ip_addresses_from_ip_addr_show_output, normalize_dns_server,
    parse_amd_rocm_smi_json, parse_cpuinfo_name, parse_ip_addr_show_output, parse_ip_address_list,
    parse_loadavg, parse_lscpu_model_name, parse_lspci_gpu_name, parse_meminfo, parse_net_dev,
    parse_net_dev_interfaces, parse_net_dev_with_filter, parse_net_static_total_between,
    parse_nvidia_smi_xml, parse_os_release_pretty_name, parse_proc_stat_cpu_sample,
    parse_public_ipv4_response, parse_public_ipv6_response, parse_soc_gpu_model,
    parse_synology_os_name, parse_uptime, proc_metrics_from_parts, proxmox_os_name_from_parts,
    reset_date_ymd, reset_timestamp_for_day, resolve_host_with_dns_server,
    sysfs_drm_gpu_name_from_driver, virtualization_from_cpuid_parts, DiskMount, MemoryValues,
    NetworkFilter, NetworkTotals,
};
use std::fs;
use std::net::UdpSocket;
use std::time::Duration;

#[test]
fn parse_loadavg_extracts_loads_and_process_count() {
    let load = parse_loadavg("0.12 0.34 0.56 3/245 98765\n").unwrap();

    assert_eq!(load.load1, 0.12);
    assert_eq!(load.load5, 0.34);
    assert_eq!(load.load15, 0.56);
    assert_eq!(load.process_count, 245);
}

#[test]
fn parse_uptime_uses_first_seconds_field() {
    assert_eq!(parse_uptime("12345.67 890.12\n").unwrap(), 12345);
}

#[test]
fn count_process_entries_matches_go_agent_numeric_proc_dirs() {
    let entries = ["1", "2", "self", "thread-self", "abc123", "42"];

    assert_eq!(count_process_entries(entries), 3);
}

#[test]
fn count_process_entries_in_dir_supports_host_proc_root() {
    let root = std::env::temp_dir().join(format!(
        "kelicloud-agent-rs-host-proc-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("1")).unwrap();
    fs::create_dir_all(root.join("42")).unwrap();
    fs::create_dir_all(root.join("self")).unwrap();
    fs::create_dir_all(root.join("abc123")).unwrap();

    let count = count_process_entries_in_dir(&root);
    let _ = fs::remove_dir_all(root);

    assert_eq!(count, 2);
}

#[test]
fn proc_metrics_uses_proc_entry_count_instead_of_loadavg_process_total() {
    let metrics = proc_metrics_from_parts(
        parse_loadavg("0.12 0.34 0.56 3/245 98765\n").unwrap(),
        123,
        NetworkTotals {
            total_up: 10,
            total_down: 20,
        },
        7,
        8,
        3,
    );

    assert_eq!(metrics.process_count, 3);
    assert_eq!(metrics.load1, 0.12);
    assert_eq!(metrics.uptime, 123);
}

#[test]
fn proc_metrics_sample_reports_go_agent_error_labels_for_missing_sources() {
    let root = temp_proc_root("missing-sources");
    fs::create_dir_all(root.join("net")).unwrap();
    fs::write(root.join("loadavg"), "0.12 0.34 0.56 1/3 99\n").unwrap();
    fs::write(root.join("uptime"), "not-a-number\n").unwrap();
    fs::write(root.join("1"), "").unwrap();

    let sample =
        collect_proc_metrics_sample_with_filter_and_proc_root(&root, &NetworkFilter::default());
    fs::remove_dir_all(&root).unwrap();

    assert_eq!(sample.metrics.uptime, 0);
    assert_eq!(sample.metrics.network.total_up, 0);
    assert_eq!(sample.metrics.tcp_connections, 0);
    assert_eq!(sample.metrics.udp_connections, 0);
    assert_eq!(
        sample.errors.network_speed.as_deref(),
        Some("failed to get network IO counters: /proc/net/dev is not readable")
    );
    assert_eq!(
        sample.errors.connections.as_deref(),
        Some("failed to get TCP connections: /proc/net/tcp is not readable")
    );
    assert_eq!(
        sample.errors.uptime.as_deref(),
        Some("/proc/uptime is not readable or invalid")
    );
}

#[test]
fn network_speed_from_samples_matches_go_agent_one_second_delta() {
    let sample = network_speed_from_samples(
        NetworkTotals {
            total_up: 1_000,
            total_down: 2_000,
        },
        NetworkTotals {
            total_up: 1_750,
            total_down: 2_600,
        },
        1.0,
    );

    assert_eq!(sample.total.total_up, 1_750);
    assert_eq!(sample.total.total_down, 2_600);
    assert_eq!(sample.speed.total_up, 750);
    assert_eq!(sample.speed.total_down, 600);
}

#[test]
fn cpu_usage_from_proc_stat_samples_matches_gopsutil_percent_formula() {
    let first = parse_proc_stat_cpu_sample(
        r#"
cpu0 999 999 999 999
cpu 100 10 40 850 20 0 10 0 0 0
"#,
    )
    .unwrap();
    let second = parse_proc_stat_cpu_sample(
        r#"
cpu 130 15 55 900 25 0 15 0 0 0
cpu0 999 999 999 999
"#,
    )
    .unwrap();

    assert_eq!(
        cpu_usage_percent_from_proc_stat_samples(first, second).unwrap(),
        50.0
    );
}

#[test]
fn parse_cpuinfo_name_matches_go_agent_proc_fallback_prefixes() {
    let contents = r#"
processor   : 0
model name  : AMD EPYC 7763 64-Core Processor
Hardware	: ARMv8 Processor rev 1
"#;

    assert_eq!(
        parse_cpuinfo_name(contents).as_deref(),
        Some("ARMv8 Processor rev 1")
    );

    assert_eq!(parse_cpuinfo_name("Processor\t: 0\n").as_deref(), Some("0"));
}

#[test]
fn parse_lscpu_model_name_matches_go_agent_cpu_name_priority() {
    let contents = r#"
Architecture:             x86_64
CPU(s):                   8
Model name:               Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz
Vendor ID:                GenuineIntel
"#;

    assert_eq!(
        parse_lscpu_model_name(contents).as_deref(),
        Some("Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz")
    );
    assert_eq!(parse_lscpu_model_name("Architecture: arm64\n"), None);
}

#[test]
fn cpu_name_from_sources_matches_go_agent_priority_and_default() {
    let lscpu = "Model name:  Neoverse-N1\n";
    let cpuinfo = "Processor\t: ARMv8 Processor rev 1\n";
    let cpuinfo_vendor_family = r#"
processor   : 0
vendor_id   : GenuineIntel
cpu family  : 6
"#;

    assert_eq!(
        cpu_name_from_sources(Some(lscpu), Some("sysinfo brand"), Some(cpuinfo)),
        "Neoverse-N1"
    );
    assert_eq!(
        cpu_name_from_sources(None, Some("sysinfo brand"), Some(cpuinfo)),
        "sysinfo brand"
    );
    assert_eq!(
        cpu_name_from_sources(None, Some("  "), Some(cpuinfo)),
        "ARMv8 Processor rev 1"
    );
    assert_eq!(
        cpu_name_from_sources(None, Some("  "), Some(cpuinfo_vendor_family)),
        "GenuineIntel 6"
    );
    assert_eq!(cpu_name_from_sources(None, None, None), "Unknown");
}

fn temp_proc_root(label: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "kelicloud-agent-rs-proc-{label}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn parse_os_release_pretty_name_matches_go_agent_os_name() {
    assert_eq!(
        parse_os_release_pretty_name(
            "NAME=Debian\nPRETTY_NAME=\"Debian GNU/Linux 12 (bookworm)\"\n"
        )
        .as_deref(),
        Some("Debian GNU/Linux 12 (bookworm)")
    );
    assert_eq!(
        parse_os_release_pretty_name("PRETTY_NAME=Alpine Linux v3.20\n").as_deref(),
        Some("Alpine Linux v3.20")
    );
}

#[test]
fn special_linux_os_names_match_go_agent_priority_helpers() {
    assert_eq!(
        proxmox_os_name_from_parts(
            "pve-manager/8.2.7/3e0176e6c0a0\n",
            "VERSION_CODENAME=bookworm\n"
        )
        .as_deref(),
        Some("Proxmox VE 8.2.7 (bookworm)")
    );
    assert_eq!(
        proxmox_os_name_from_parts("", "VERSION_CODENAME=bookworm\n").as_deref(),
        Some("Proxmox VE")
    );
    assert_eq!(
        parse_synology_os_name(
            r#"
unique="synology_apollolake_918+"
udc_check_state="7.2.1"
"#
        )
        .as_deref(),
        Some("Synology 918+ DSM 7.2.1")
    );
    assert_eq!(parse_synology_os_name(r#"unique="synology_ds""#), None);
    assert_eq!(
        fnos_os_name_from_markers(Some("1.1.11\n"), true).as_deref(),
        Some("fnOS 1.1.11")
    );
    assert_eq!(
        fnos_os_name_from_markers(None, true).as_deref(),
        Some("fnOS")
    );
    assert_eq!(
        android_os_name_from_build_prop(
            r#"
ro.build.version.release=14
ro.product.model=Pixel 8
ro.product.brand=Google
"#
        )
        .as_deref(),
        Some("Android 14 (Google Pixel 8)")
    );
    assert_eq!(
        android_os_name_from_build_prop("ro.product.model=Pixel 8\n").as_deref(),
        Some("Android")
    );
}

#[test]
fn kernel_version_matches_go_agent_uname_fallback() {
    assert_eq!(
        kernel_version_from_uname_output(Some("6.8.0-60-generic\n")),
        "6.8.0-60-generic"
    );
    assert_eq!(kernel_version_from_uname_output(None), "Unknown");
}

#[test]
fn parse_ip_address_list_picks_first_ipv4_and_global_ipv6() {
    let parsed = parse_ip_address_list("127.0.0.1 10.0.0.5 fe80::1 2607:f358:1a:e::ab0:39b7");

    assert_eq!(parsed.ipv4, "10.0.0.5");
    assert_eq!(parsed.ipv6, "2607:f358:1a:e::ab0:39b7");
}

#[test]
fn parse_ip_addr_show_output_filters_interfaces_like_go_agent_nic_ip_mode() {
    let contents = r#"
1: lo    inet 127.0.0.1/8 scope host lo\       valid_lft forever preferred_lft forever
2: eth0    inet 10.0.0.5/24 brd 10.0.0.255 scope global eth0\       valid_lft forever preferred_lft forever
2: eth0    inet6 fe80::1/64 scope link \       valid_lft forever preferred_lft forever
2: eth0    inet6 2607:f358:1a:e::ab0:39b7/64 scope global \       valid_lft forever preferred_lft forever
3: ens18    inet 203.0.113.9/24 brd 203.0.113.255 scope global ens18\       valid_lft forever preferred_lft forever
"#;

    let include_eth0 = parse_ip_addr_show_output(contents, &NetworkFilter::from_csv("eth0", ""));
    assert_eq!(include_eth0.ipv4, "10.0.0.5");
    assert_eq!(include_eth0.ipv6, "2607:f358:1a:e::ab0:39b7");

    let exclude_eth0 = parse_ip_addr_show_output(contents, &NetworkFilter::from_csv("", "eth0"));
    assert_eq!(exclude_eth0.ipv4, "203.0.113.9");
    assert!(exclude_eth0.ipv6.is_empty());
}

#[test]
fn parse_ip_addr_show_output_handles_real_multiline_interface_blocks() {
    let contents = r#"
1: lo: <LOOPBACK,UP,LOWER_UP> mtu 65536 qdisc noqueue state UNKNOWN group default qlen 1000
    inet 127.0.0.1/8 scope host lo
       valid_lft forever preferred_lft forever
2: eth0: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 qdisc mq state UP group default qlen 1000
    inet 10.0.0.5/24 brd 10.0.0.255 scope global eth0
       valid_lft forever preferred_lft forever
    inet6 fe80::1/64 scope link
       valid_lft forever preferred_lft forever
    inet6 2607:f358:1a:e::ab0:39b7/64 scope global dynamic
       valid_lft 2591999sec preferred_lft 604799sec
3: ens18: <BROADCAST,MULTICAST> mtu 1500 qdisc mq state DOWN group default qlen 1000
    inet 203.0.113.9/24 brd 203.0.113.255 scope global ens18
"#;

    let include_eth0 = parse_ip_addr_show_output(contents, &NetworkFilter::from_csv("eth0", ""));
    assert_eq!(include_eth0.ipv4, "10.0.0.5");
    assert_eq!(include_eth0.ipv6, "2607:f358:1a:e::ab0:39b7");

    let exclude_eth0 = parse_ip_addr_show_output(contents, &NetworkFilter::from_csv("", "eth0"));
    assert!(exclude_eth0.ipv4.is_empty());
    assert!(exclude_eth0.ipv6.is_empty());
}

#[test]
fn nic_ip_addresses_from_ip_addr_show_output_returns_none_when_filtered_nics_have_no_ips() {
    let contents = r#"
2: eth0: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 qdisc mq state UP group default qlen 1000
    inet 10.0.0.5/24 brd 10.0.0.255 scope global eth0
3: ens18: <BROADCAST,MULTICAST> mtu 1500 qdisc mq state DOWN group default qlen 1000
    inet 203.0.113.9/24 brd 203.0.113.255 scope global ens18
"#;

    let parsed =
        nic_ip_addresses_from_ip_addr_show_output(contents, &NetworkFilter::from_csv("ens18", ""));

    assert_eq!(parsed, None);
}

#[test]
fn parse_public_ipv4_response_extracts_ips_from_go_agent_api_shapes() {
    assert_eq!(
        parse_public_ipv4_response("ip=203.0.113.10\nloc=US\n").as_deref(),
        Some("203.0.113.10")
    );
    assert_eq!(
        parse_public_ipv4_response(r#"{"ip":"198.51.100.24"}"#).as_deref(),
        Some("198.51.100.24")
    );
    assert_eq!(
        parse_public_ipv4_response(r#"{"ip":"999.999.999.999","real":"198.51.100.24"}"#).as_deref(),
        Some("999.999.999.999")
    );
}

#[test]
fn parse_public_ipv6_response_matches_go_agent_first_regex_match() {
    assert_eq!(
        parse_public_ipv6_response(r#"{"ip":"fe80::1","real":"2607:f358:1a:e::ab0:39b7"}"#)
            .as_deref(),
        Some("fe80::1")
    );
    assert_eq!(
        parse_public_ipv6_response("ip=2001:db8:0:1:2:3:4:5:6\n").as_deref(),
        Some("2001:db8:0:1:2:3:4:5")
    );
}

#[test]
fn normalize_dns_server_matches_go_agent_custom_dns_flag() {
    assert_eq!(normalize_dns_server("1.1.1.1"), "1.1.1.1:53");
    assert_eq!(normalize_dns_server("8.8.8.8:5353"), "8.8.8.8:5353");
    assert_eq!(
        normalize_dns_server("2606:4700:4700::1111"),
        "[2606:4700:4700::1111]:53"
    );
    assert_eq!(
        normalize_dns_server("[2606:4700:4700::1111]:5353"),
        "[2606:4700:4700::1111]:5353"
    );
}

#[test]
fn resolve_host_with_dns_server_uses_the_configured_dns_endpoint() {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let server = socket.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        for _ in 0..2 {
            let mut request = [0_u8; 512];
            let (len, peer) = socket.recv_from(&mut request).unwrap();
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
                response.extend_from_slice(&[203, 0, 113, 10]);
            }

            socket.send_to(&response, peer).unwrap();
        }
    });

    let addrs =
        resolve_host_with_dns_server(&server.to_string(), "example.com", Duration::from_secs(1))
            .unwrap();

    assert!(addrs.contains(&"203.0.113.10:0".parse().unwrap()));
    handle.join().unwrap();
}

#[test]
fn detect_container_from_cgroup_matches_common_runtime_markers() {
    let docker = "0::/system.slice/docker-0123456789abcdef.scope\n";
    let kube =
        "0::/kubepods.slice/kubepods-burstable-pod123e4567-e89b-12d3-a456-426614174000.slice\n";

    assert_eq!(
        detect_container_from_cgroup(docker).as_deref(),
        Some("docker")
    );
    assert_eq!(
        detect_container_from_cgroup(kube).as_deref(),
        Some("kubernetes")
    );
    assert_eq!(
        detect_container_from_cgroup("0::/system.slice/docker/service\n"),
        None
    );
    assert_eq!(
        detect_container_from_cgroup("0::/kubepods.slice/kubepods-burstable.slice\n"),
        None
    );
    assert_eq!(
        detect_container_from_cgroup("0::/system.slice/crio-monitor.scope\n"),
        None
    );
}

#[test]
fn detect_container_from_markers_prefers_runtime_for_containerenv_like_go_agent() {
    let podman = "0::/user.slice/libpod-0123456789abcdef0123456789abcdef.scope\n";

    assert_eq!(
        detect_container_from_markers(false, true, false, Some(podman)).as_deref(),
        Some("podman")
    );
    assert_eq!(
        detect_container_from_markers(false, true, false, Some("0::/user.slice/session-2.scope\n"))
            .as_deref(),
        Some("container")
    );
    assert_eq!(
        detect_container_from_markers(true, true, false, Some(podman)).as_deref(),
        Some("docker")
    );
    assert_eq!(
        detect_container_from_markers(false, false, true, None).as_deref(),
        Some("container")
    );
}

#[test]
fn virtualization_from_cpuid_parts_matches_go_agent_vendor_mapping() {
    assert_eq!(virtualization_from_cpuid_parts(false, "KVMKVMKVM"), "none");
    assert_eq!(virtualization_from_cpuid_parts(true, "KVMKVMKVM"), "kvm");
    assert_eq!(
        virtualization_from_cpuid_parts(true, "Microsoft Hv"),
        "microsoft"
    );
    assert_eq!(
        virtualization_from_cpuid_parts(true, "VMwareVMware"),
        "vmware"
    );
    assert_eq!(
        virtualization_from_cpuid_parts(true, "VBoxVBoxVBox"),
        "oracle"
    );
    assert_eq!(virtualization_from_cpuid_parts(true, ""), "virtualized");
    assert_eq!(
        virtualization_from_cpuid_parts(true, "FancyCloud"),
        "fancycloud"
    );
}

#[test]
fn parse_lspci_gpu_name_prefers_real_gpu_and_excludes_virtual_display() {
    let contents = r#"
00:02.0 VGA compatible controller: Cirrus Logic GD 5446
00:03.0 Ethernet controller: Intel Corporation 82540EM Gigabit Ethernet Controller
01:00.0 3D controller: NVIDIA Corporation GA102 [GeForce RTX 3090] (rev a1)
02:00.0 Display controller: VMware SVGA II Adapter
"#;

    assert_eq!(
        parse_lspci_gpu_name(contents).as_deref(),
        Some("NVIDIA Corporation GA102 [GeForce RTX 3090]")
    );
}

#[test]
fn parse_lspci_gpu_name_uses_go_agent_line_order_for_priority_vendors() {
    let contents = r#"
00:02.0 VGA compatible controller: Intel Corporation UHD Graphics 770
01:00.0 3D controller: NVIDIA Corporation AD102 [GeForce RTX 4090] (rev a1)
"#;

    assert_eq!(
        parse_lspci_gpu_name(contents).as_deref(),
        Some("Intel Corporation UHD Graphics 770")
    );
}

#[test]
fn parse_lspci_gpu_name_strips_any_trailing_parenthesized_suffix_like_go_agent() {
    let contents = r#"
00:02.0 VGA compatible controller: Intel Corporation UHD Graphics (Mobile)
"#;

    assert_eq!(
        parse_lspci_gpu_name(contents).as_deref(),
        Some("Intel Corporation UHD Graphics")
    );
}

#[test]
fn parse_lspci_gpu_name_skips_malformed_display_lines_like_go_agent() {
    let contents = r#"
00:02.0 VGA compatible controller Intel Corporation:
01:00.0 3D controller: NVIDIA Corporation GA102 [GeForce RTX 3090] (rev a1)
"#;

    assert_eq!(
        parse_lspci_gpu_name(contents).as_deref(),
        Some("NVIDIA Corporation GA102 [GeForce RTX 3090]")
    );
}

#[test]
fn sysfs_drm_gpu_name_matches_go_agent_arm_soc_and_driver_fallbacks() {
    assert_eq!(
        parse_soc_gpu_model("msm", b"qcom,adreno-750.1\0qcom,adreno").as_deref(),
        Some("Qualcomm Adreno 750")
    );
    assert_eq!(
        parse_soc_gpu_model("panfrost", b"rockchip,mali-g610").as_deref(),
        Some("ARM Mali G610")
    );
    assert_eq!(
        parse_soc_gpu_model("vc4", b"brcm,bcm2711-vc5").as_deref(),
        Some("Broadcom VideoCore VI (Pi 4)")
    );
    assert_eq!(
        sysfs_drm_gpu_name_from_driver("virtio_gpu", None).as_deref(),
        None
    );
    assert_eq!(
        sysfs_drm_gpu_name_from_driver("i915", None).as_deref(),
        Some("Intel Integrated Graphics")
    );
    assert_eq!(
        sysfs_drm_gpu_name_from_driver("unknown_drm", None).as_deref(),
        Some("Direct Render Manager unknown_drm")
    );
}

#[test]
fn parse_nvidia_smi_xml_extracts_detailed_gpu_metrics() {
    let xml = r#"
<nvidia_smi_log>
  <gpu>
    <product_name>NVIDIA GeForce RTX 3090</product_name>
    <fb_memory_usage>
      <total>24576 MiB</total>
      <used>1024 MiB</used>
    </fb_memory_usage>
    <utilization>
      <gpu_util>25 %</gpu_util>
    </utilization>
    <temperature>
      <gpu_temp>65 C</gpu_temp>
    </temperature>
  </gpu>
</nvidia_smi_log>
"#;

    let metrics = parse_nvidia_smi_xml(xml);

    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0].name, "NVIDIA GeForce RTX 3090");
    assert_eq!(metrics[0].memory_total, 24_576 * 1024 * 1024);
    assert_eq!(metrics[0].memory_used, 1024 * 1024 * 1024);
    assert_eq!(metrics[0].utilization, 25.0);
    assert_eq!(metrics[0].temperature, 65);
}

#[test]
fn parse_nvidia_smi_xml_accepts_gpu_tags_with_attributes_like_go_agent() {
    let xml = r#"
<nvidia_smi_log>
  <gpu id="00000000:01:00.0">
    <product_name>NVIDIA GeForce RTX 4090</product_name>
    <fb_memory_usage>
      <total>24564 MiB</total>
      <used>2048 MiB</used>
    </fb_memory_usage>
    <utilization>
      <gpu_util>17 %</gpu_util>
    </utilization>
    <temperature>
      <gpu_temp>42 C</gpu_temp>
    </temperature>
  </gpu>
</nvidia_smi_log>
"#;

    let metrics = parse_nvidia_smi_xml(xml);

    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0].name, "NVIDIA GeForce RTX 4090");
    assert_eq!(metrics[0].memory_total, 24_564 * 1024 * 1024);
    assert_eq!(metrics[0].memory_used, 2048 * 1024 * 1024);
    assert_eq!(metrics[0].utilization, 17.0);
    assert_eq!(metrics[0].temperature, 42);
}

#[test]
fn parse_nvidia_smi_xml_uses_framebuffer_memory_like_go_agent() {
    let xml = r#"
<nvidia_smi_log>
  <gpu>
    <product_name>NVIDIA A100</product_name>
    <bar1_memory_usage>
      <total>16384 MiB</total>
      <used>1024 MiB</used>
    </bar1_memory_usage>
    <fb_memory_usage>
      <total>40960 MiB</total>
      <used>20480 MiB</used>
    </fb_memory_usage>
    <utilization>
      <gpu_util>50 %</gpu_util>
    </utilization>
    <temperature>
      <gpu_temp>58 C</gpu_temp>
    </temperature>
  </gpu>
</nvidia_smi_log>
"#;

    let metrics = parse_nvidia_smi_xml(xml);

    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0].memory_total, 40_960 * 1024 * 1024);
    assert_eq!(metrics[0].memory_used, 20_480 * 1024 * 1024);
}

#[test]
fn parse_nvidia_smi_xml_uses_structured_utilization_and_temperature_like_go_agent() {
    let xml = r#"
<nvidia_smi_log>
  <gpu>
    <product_name>NVIDIA A100</product_name>
    <diagnostics>
      <gpu_util>99 %</gpu_util>
      <gpu_temp>99 C</gpu_temp>
    </diagnostics>
    <fb_memory_usage>
      <total>40960 MiB</total>
      <used>20480 MiB</used>
    </fb_memory_usage>
    <utilization>
      <gpu_util>50 %</gpu_util>
    </utilization>
    <temperature>
      <gpu_temp>58 C</gpu_temp>
    </temperature>
  </gpu>
</nvidia_smi_log>
"#;

    let metrics = parse_nvidia_smi_xml(xml);

    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0].utilization, 50.0);
    assert_eq!(metrics[0].temperature, 58);
}

#[test]
fn parse_amd_rocm_smi_json_extracts_detailed_gpu_metrics() {
    let json = r#"
{
  "card0": {
    "Card series": "AMD Radeon RX 7900 XTX",
    "GPU use (%)": "31",
    "VRAM Total Memory (B)": "25769803776",
    "VRAM Total Used Memory (B)": "2147483648",
    "Temperature (Sensor junction) (C)": "70"
  }
}
"#;

    let metrics = parse_amd_rocm_smi_json(json);

    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0].name, "AMD Radeon RX 7900 XTX");
    assert_eq!(metrics[0].memory_total, 25_769_803_776);
    assert_eq!(metrics[0].memory_used, 2_147_483_648);
    assert_eq!(metrics[0].utilization, 31.0);
    assert_eq!(metrics[0].temperature, 70);
}

#[test]
fn detailed_gpu_parsers_keep_entries_with_missing_names_like_go_agent() {
    let nvidia_xml = r#"
<nvidia_smi_log>
  <gpu>
    <fb_memory_usage>
      <total>8192 MiB</total>
      <used>512 MiB</used>
    </fb_memory_usage>
    <utilization>
      <gpu_util>7 %</gpu_util>
    </utilization>
    <temperature>
      <gpu_temp>41 C</gpu_temp>
    </temperature>
  </gpu>
</nvidia_smi_log>
"#;
    let nvidia_metrics = parse_nvidia_smi_xml(nvidia_xml);

    assert_eq!(nvidia_metrics.len(), 1);
    assert_eq!(nvidia_metrics[0].name, "");
    assert_eq!(nvidia_metrics[0].memory_total, 8192 * 1024 * 1024);
    assert_eq!(nvidia_metrics[0].memory_used, 512 * 1024 * 1024);
    assert_eq!(nvidia_metrics[0].utilization, 7.0);
    assert_eq!(nvidia_metrics[0].temperature, 41);

    let amd_json = r#"
{
  "card0": {
    "GPU use (%)": "9",
    "VRAM Total Memory (B)": "17179869184",
    "VRAM Total Used Memory (B)": "268435456",
    "Temperature (Sensor junction) (C)": "44"
  }
}
"#;
    let amd_metrics = parse_amd_rocm_smi_json(amd_json);

    assert_eq!(amd_metrics.len(), 1);
    assert_eq!(amd_metrics[0].name, "");
    assert_eq!(amd_metrics[0].memory_total, 17_179_869_184);
    assert_eq!(amd_metrics[0].memory_used, 268_435_456);
    assert_eq!(amd_metrics[0].utilization, 9.0);
    assert_eq!(amd_metrics[0].temperature, 44);
}

#[test]
fn detailed_gpu_parsers_zero_negative_unsigned_fields_like_go_agent() {
    let nvidia_xml = r#"
<nvidia_smi_log>
  <gpu>
    <product_name>NVIDIA Test GPU</product_name>
    <fb_memory_usage>
      <total>-8192 MiB</total>
      <used>-512 MiB</used>
    </fb_memory_usage>
    <utilization>
      <gpu_util>-7 %</gpu_util>
    </utilization>
    <temperature>
      <gpu_temp>-41 C</gpu_temp>
    </temperature>
  </gpu>
</nvidia_smi_log>
"#;
    let nvidia_metrics = parse_nvidia_smi_xml(nvidia_xml);

    assert_eq!(nvidia_metrics.len(), 1);
    assert_eq!(nvidia_metrics[0].memory_total, 0);
    assert_eq!(nvidia_metrics[0].memory_used, 0);
    assert_eq!(nvidia_metrics[0].utilization, -7.0);
    assert_eq!(nvidia_metrics[0].temperature, 0);

    let amd_json = r#"
{
  "card0": {
    "Card series": "AMD Test GPU",
    "GPU use (%)": "-9",
    "VRAM Total Memory (B)": "-17179869184",
    "VRAM Total Used Memory (B)": "-268435456",
    "Temperature (Sensor junction) (C)": "-44"
  }
}
"#;
    let amd_metrics = parse_amd_rocm_smi_json(amd_json);

    assert_eq!(amd_metrics.len(), 1);
    assert_eq!(amd_metrics[0].memory_total, 0);
    assert_eq!(amd_metrics[0].memory_used, 0);
    assert_eq!(amd_metrics[0].utilization, -9.0);
    assert_eq!(amd_metrics[0].temperature, 0);
}

#[test]
fn parse_meminfo_calculates_go_compatible_memory() {
    let contents = r#"
MemTotal:        1000 kB
MemFree:          100 kB
MemAvailable:     700 kB
Buffers:           25 kB
Cached:           200 kB
SwapCached:        25 kB
Shmem:             10 kB
SReclaimable:      50 kB
SwapTotal:        500 kB
SwapFree:         100 kB
"#;

    let meminfo = parse_meminfo(contents);
    let ram = go_compatible_ram(&meminfo);
    let swap = go_compatible_swap(&meminfo);

    assert_eq!(ram.total, 1000 * 1024);
    assert_eq!(ram.used, 635 * 1024);
    assert_eq!(swap.total, 500 * 1024);
    assert_eq!(swap.used, 375 * 1024);
}

#[test]
fn parse_meminfo_ignores_negative_values_like_go_agent() {
    let meminfo = parse_meminfo(
        r#"
MemTotal:        -1000 kB
MemFree:         -100 kB
Buffers:         -25 kB
Cached:          -200 kB
SwapTotal:       -500 kB
SwapFree:        -100 kB
SwapCached:      -25 kB
Shmem:           -10 kB
SReclaimable:    -50 kB
"#,
    );

    assert_eq!(meminfo.mem_total, 0);
    assert_eq!(meminfo.mem_free, 0);
    assert_eq!(meminfo.buffers, 0);
    assert_eq!(meminfo.cached, 0);
    assert_eq!(meminfo.swap_total, 0);
    assert_eq!(meminfo.swap_free, 0);
    assert_eq!(meminfo.swap_cached, 0);
    assert_eq!(meminfo.shmem, 0);
    assert_eq!(meminfo.s_reclaimable, 0);
}

#[test]
fn memory_values_from_meminfo_rejects_zero_total_like_go_agent_fallback() {
    let meminfo = parse_meminfo(
        r#"
MemFree:          100 kB
Buffers:           25 kB
Cached:           200 kB
Shmem:             10 kB
SwapTotal:        500 kB
SwapFree:         100 kB
"#,
    );

    assert_eq!(
        memory_values_from_meminfo_with_modes(&meminfo, false, false),
        None
    );
}

#[test]
fn memory_selection_keeps_swap_from_meminfo_when_ram_needs_fallback_like_go_agent() {
    let meminfo = parse_meminfo(
        r#"
MemFree:          100 kB
Buffers:           25 kB
Cached:           200 kB
Shmem:             10 kB
SwapTotal:        500 kB
SwapFree:         100 kB
SwapCached:        25 kB
"#,
    );

    let selection = memory_selection_from_meminfo_with_modes(&meminfo, false, false);

    assert_eq!(selection.ram, None);
    assert_eq!(
        selection.swap,
        MemoryValues {
            total: 500 * 1024,
            used: 375 * 1024,
        }
    );
}

#[test]
fn memory_raw_used_mode_reports_zero_ram_when_memtotal_missing_like_go_agent() {
    let meminfo = parse_meminfo(
        r#"
MemFree:          100 kB
Buffers:           25 kB
Cached:           200 kB
Shmem:             10 kB
SwapTotal:        500 kB
SwapFree:         100 kB
"#,
    );

    let selection = memory_selection_from_meminfo_with_modes(&meminfo, false, true);

    assert_eq!(selection.ram, Some(MemoryValues::default()));
    assert_eq!(
        selection.swap,
        MemoryValues {
            total: 500 * 1024,
            used: 400 * 1024,
        }
    );
}

#[test]
fn memory_include_cache_mode_matches_go_agent_flag() {
    let meminfo = parse_meminfo(
        r#"
MemTotal:        1000 kB
MemFree:          100 kB
Buffers:           25 kB
Cached:           200 kB
SReclaimable:      50 kB
Shmem:             10 kB
"#,
    );

    let ram = go_compatible_ram_include_cache(&meminfo);

    assert_eq!(ram.total, 1000 * 1024);
    assert_eq!(ram.used, 900 * 1024);
}

#[test]
fn memory_raw_used_mode_matches_go_agent_htop_like_branch() {
    let meminfo = parse_meminfo(
        r#"
MemTotal:        1000 kB
MemFree:          100 kB
Buffers:           25 kB
Cached:           200 kB
SReclaimable:      50 kB
Shmem:             10 kB
"#,
    );

    let ram = go_compatible_ram_raw_used(&meminfo);

    assert_eq!(ram.total, 1000 * 1024);
    assert_eq!(ram.used, 635 * 1024);
}

#[test]
fn parse_net_dev_excludes_loopback_and_container_interfaces() {
    let contents = r#"
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 50 1 0 0 0 0 0 0 60 1 0 0 0 0 0 0
  eth0: 1000 10 0 0 0 0 0 0 2000 20 0 0 0 0 0 0
docker0: 3000 30 0 0 0 0 0 0 4000 40 0 0 0 0 0 0
 ens18: 500 5 0 0 0 0 0 0 700 7 0 0 0 0 0 0
"#;

    let totals = parse_net_dev(contents);

    assert_eq!(totals.total_down, 1500);
    assert_eq!(totals.total_up, 2700);
}

#[test]
fn parse_net_dev_honors_include_and_exclude_filters() {
    let contents = r#"
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
  eth0: 1000 10 0 0 0 0 0 0 2000 20 0 0 0 0 0 0
 ens18: 500 5 0 0 0 0 0 0 700 7 0 0 0 0 0 0
 ens19: 300 3 0 0 0 0 0 0 400 4 0 0 0 0 0 0
"#;

    let include = NetworkFilter::from_csv("ens18,ens19", "");
    let excluded = NetworkFilter::from_csv("", "ens19");

    assert_eq!(
        parse_net_dev_with_filter(contents, &include).total_down,
        800
    );
    assert_eq!(parse_net_dev_with_filter(contents, &include).total_up, 1100);
    assert_eq!(
        parse_net_dev_with_filter(contents, &excluded).total_down,
        1500
    );
    assert_eq!(
        parse_net_dev_with_filter(contents, &excluded).total_up,
        2700
    );
}

#[test]
fn network_filter_treats_whitespace_include_list_like_go_agent_whitelist() {
    let contents = r#"
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
  eth0: 1000 10 0 0 0 0 0 0 2000 20 0 0 0 0 0 0
"#;

    let totals = parse_net_dev_with_filter(contents, &NetworkFilter::from_csv(" ", ""));

    assert_eq!(totals.total_down, 0);
    assert_eq!(totals.total_up, 0);
}

#[test]
fn parse_net_dev_interfaces_returns_filtered_per_nic_counters() {
    let contents = r#"
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 100 0 0 0 0 0 0 0 200 0 0 0 0 0 0 0
  eth0: 800 0 0 0 0 0 0 0 1100 0 0 0 0 0 0 0
 ens19: 700 0 0 0 0 0 0 0 1600 0 0 0 0 0 0 0
"#;
    let filter = NetworkFilter::from_csv("eth0", "");

    let counters = parse_net_dev_interfaces(contents, &filter);

    assert_eq!(counters.len(), 1);
    assert_eq!(counters[0].name, "eth0");
    assert_eq!(counters[0].total_down, 800);
    assert_eq!(counters[0].total_up, 1100);
}

#[test]
fn parse_net_static_total_between_sums_reset_window_and_filters_nics() {
    let contents = r#"
{
  "interfaces": {
    "eth0": [
      {"timestamp": 100, "tx": 10, "rx": 20},
      {"timestamp": 200, "tx": 30, "rx": 40}
    ],
    "ens18": [
      {"timestamp": 150, "tx": 5, "rx": 6}
    ],
    "docker0": [
      {"timestamp": 200, "tx": 999, "rx": 999}
    ]
  }
}
"#;
    let filter = NetworkFilter::from_csv("eth0,ens18", "");

    let totals = parse_net_static_total_between(contents, 150, 250, &filter).unwrap();

    assert_eq!(totals.total_up, 35);
    assert_eq!(totals.total_down, 46);
}

#[test]
fn reset_date_ymd_matches_go_agent_month_rotate_rules() {
    assert_eq!(reset_date_ymd(15, 2026, 6, 20), (2026, 6, 15));
    assert_eq!(reset_date_ymd(15, 2026, 6, 10), (2026, 5, 15));
    assert_eq!(reset_date_ymd(31, 2026, 3, 1), (2026, 3, 1));
    assert_eq!(reset_date_ymd(0, 2026, 6, 20), (2026, 6, 20));
}

#[test]
fn reset_timestamp_for_day_uses_local_midnight_reset_window() {
    let now = Local.with_ymd_and_hms(2026, 6, 20, 12, 0, 0).unwrap();
    let expected = Local
        .with_ymd_and_hms(2026, 6, 15, 0, 0, 0)
        .unwrap()
        .timestamp() as u64;

    assert_eq!(reset_timestamp_for_day(15, now), Some(expected));
}

#[test]
fn disk_mounts_filter_like_go_agent() {
    let mounts = vec![
        DiskMount {
            device: "/dev/loop0".to_string(),
            mountpoint: "/".to_string(),
            fstype: "overlay".to_string(),
            total: 1_000,
            used: 400,
        },
        DiskMount {
            device: "tmpfs".to_string(),
            mountpoint: "/run".to_string(),
            fstype: "tmpfs".to_string(),
            total: 9_999,
            used: 9_999,
        },
        DiskMount {
            device: "overlay".to_string(),
            mountpoint: "/var/lib/docker/overlay2".to_string(),
            fstype: "overlay".to_string(),
            total: 9_999,
            used: 9_999,
        },
        DiskMount {
            device: "/dev/sdb1".to_string(),
            mountpoint: "/data".to_string(),
            fstype: "ext4".to_string(),
            total: 2_000,
            used: 100,
        },
        DiskMount {
            device: "tank/app".to_string(),
            mountpoint: "/tank/app".to_string(),
            fstype: "zfs".to_string(),
            total: 3_000,
            used: 1_000,
        },
        DiskMount {
            device: "tank/logs".to_string(),
            mountpoint: "/tank/logs".to_string(),
            fstype: "zfs".to_string(),
            total: 2_500,
            used: 800,
        },
    ];

    let disk = go_compatible_disk(&mounts);

    assert_eq!(disk.total, 6_000);
    assert_eq!(disk.used, 1_500);
}

#[test]
fn disk_mounts_honor_include_mountpoints_like_go_agent() {
    let mounts = vec![
        DiskMount {
            device: "/dev/sda1".to_string(),
            mountpoint: "/".to_string(),
            fstype: "ext4".to_string(),
            total: 1_000,
            used: 400,
        },
        DiskMount {
            device: "tmpfs".to_string(),
            mountpoint: "/run".to_string(),
            fstype: "tmpfs".to_string(),
            total: 2_000,
            used: 100,
        },
        DiskMount {
            device: "/dev/sdb1".to_string(),
            mountpoint: "/data".to_string(),
            fstype: "ext4".to_string(),
            total: 3_000,
            used: 500,
        },
    ];

    let disk =
        kelicloud_agent_rs::linux_proc::go_compatible_disk_with_mountpoints(&mounts, "/run;/data");

    assert_eq!(disk.total, 5_000);
    assert_eq!(disk.used, 600);
}

#[test]
fn disk_include_mountpoints_uses_direct_usage_lookup_like_go_agent() {
    let disk = kelicloud_agent_rs::linux_proc::go_compatible_disk_from_mountpoint_lookup(
        "/missing;/data;/backup",
        |mountpoint| match mountpoint {
            "/data" => Some(kelicloud_agent_rs::linux_proc::DiskValues {
                total: 3_000,
                used: 500,
            }),
            "/backup" => Some(kelicloud_agent_rs::linux_proc::DiskValues {
                total: 4_000,
                used: 1_000,
            }),
            _ => None,
        },
    );

    assert_eq!(disk.total, 7_000);
    assert_eq!(disk.used, 1_500);
}

#[test]
fn count_socket_entries_ignores_header() {
    let contents = r#"
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:0035 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 100 1 0000000000000000 100 0 0 10 0
   1: 00000000:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 101 1 0000000000000000 100 0 0 10 0
"#;

    assert_eq!(count_socket_entries(contents), 2);
}

#[test]
fn linux_supported_matches_compile_target() {
    assert_eq!(linux_supported(), cfg!(target_os = "linux"));
}
