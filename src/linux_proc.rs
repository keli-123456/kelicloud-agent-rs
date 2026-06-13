use std::collections::HashMap;
use std::error::Error;
#[cfg(target_os = "linux")]
use std::ffi::CString;
use std::fs;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Datelike, Local, TimeZone};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NetworkTotals {
    pub total_up: i64,
    pub total_down: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NetworkSpeedSample {
    pub total: NetworkTotals,
    pub speed: NetworkTotals,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkInterfaceTotals {
    pub name: String,
    pub total_up: i64,
    pub total_down: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemoryValues {
    pub total: i64,
    pub used: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemorySelection {
    pub ram: Option<MemoryValues>,
    pub swap: MemoryValues,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiskValues {
    pub total: i64,
    pub used: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiskMount {
    pub device: String,
    pub mountpoint: String,
    pub fstype: String,
    pub total: i64,
    pub used: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IpAddresses {
    pub ipv4: String,
    pub ipv6: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GpuMetric {
    pub name: String,
    pub memory_total: i64,
    pub memory_used: i64,
    pub utilization: f64,
    pub temperature: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkFilter {
    include: Vec<String>,
    exclude: Vec<String>,
}

impl NetworkFilter {
    pub fn from_csv(include: &str, exclude: &str) -> Self {
        Self {
            include: parse_csv_list(include),
            exclude: parse_csv_list(exclude),
        }
    }

    pub fn should_include(&self, name: &str) -> bool {
        if !should_include_interface(name) {
            return false;
        }
        if !self.include.is_empty() {
            return self.include.iter().any(|item| item == name);
        }
        !self.exclude.iter().any(|item| item == name)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProcMemInfo {
    pub mem_total: i64,
    pub mem_free: i64,
    pub mem_available: i64,
    pub buffers: i64,
    pub cached: i64,
    pub swap_total: i64,
    pub swap_free: i64,
    pub swap_cached: i64,
    pub shmem: i64,
    pub s_reclaimable: i64,
    pub zswap: i64,
    pub zswapped: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LoadAverage {
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
    pub process_count: i32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProcStatCpuSample {
    pub total: u64,
    pub idle: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ProcMetrics {
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
    pub uptime: i64,
    pub process_count: i32,
    pub network: NetworkTotals,
    pub tcp_connections: i32,
    pub udp_connections: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcMetricErrors {
    pub network_speed: Option<String>,
    pub connections: Option<String>,
    pub uptime: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProcMetricsSample {
    pub metrics: ProcMetrics,
    pub errors: ProcMetricErrors,
}

pub fn linux_supported() -> bool {
    cfg!(target_os = "linux")
}

pub fn parse_loadavg(contents: &str) -> Option<LoadAverage> {
    let mut fields = contents.split_whitespace();
    let load1 = fields.next()?.parse::<f64>().ok()?;
    let load5 = fields.next()?.parse::<f64>().ok()?;
    let load15 = fields.next()?.parse::<f64>().ok()?;
    let process_count = fields
        .next()
        .and_then(|value| value.split_once('/'))
        .and_then(|(_, total)| total.parse::<i32>().ok())
        .unwrap_or(0);

    Some(LoadAverage {
        load1,
        load5,
        load15,
        process_count,
    })
}

pub fn parse_uptime(contents: &str) -> Option<i64> {
    let seconds = contents.split_whitespace().next()?.parse::<f64>().ok()?;
    Some(seconds.floor() as i64)
}

pub fn parse_proc_stat_cpu_sample(contents: &str) -> Option<ProcStatCpuSample> {
    for line in contents.lines().map(str::trim) {
        let Some(fields) = line.strip_prefix("cpu ") else {
            continue;
        };
        let ticks = fields
            .split_whitespace()
            .map(str::parse::<u64>)
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        if ticks.len() < 4 {
            return None;
        }

        let total = ticks.iter().sum::<u64>();
        if total == 0 {
            return None;
        }
        let idle = ticks[3] + ticks.get(4).copied().unwrap_or(0);
        return Some(ProcStatCpuSample { total, idle });
    }

    None
}

pub fn cpu_usage_percent_from_proc_stat_samples(
    first: ProcStatCpuSample,
    second: ProcStatCpuSample,
) -> Option<f64> {
    if second.total <= first.total || second.idle < first.idle {
        return None;
    }

    let total_delta = second.total - first.total;
    if total_delta == 0 {
        return None;
    }
    let idle_delta = second.idle - first.idle;
    if idle_delta > total_delta {
        return None;
    }

    Some(((total_delta - idle_delta) as f64 / total_delta as f64) * 100.0)
}

pub fn count_process_entries<I, S>(entry_names: I) -> i32
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    entry_names
        .into_iter()
        .filter(|name| name.as_ref().parse::<i64>().is_ok())
        .count() as i32
}

pub fn count_process_entries_in_dir<P: AsRef<Path>>(path: P) -> i32 {
    fs::read_dir(path)
        .ok()
        .map(|entries| {
            count_process_entries(entries.filter_map(|entry| {
                entry
                    .ok()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
            }))
        })
        .unwrap_or(0)
}

pub fn proc_metrics_from_parts(
    load: LoadAverage,
    uptime: i64,
    network: NetworkTotals,
    tcp_connections: i32,
    udp_connections: i32,
    process_count: i32,
) -> ProcMetrics {
    ProcMetrics {
        load1: load.load1,
        load5: load.load5,
        load15: load.load15,
        uptime,
        process_count,
        network,
        tcp_connections,
        udp_connections,
    }
}

pub fn parse_cpuinfo_name(contents: &str) -> Option<String> {
    let mut vendor_id = String::new();
    let mut family = String::new();

    for line in contents.lines() {
        if line.starts_with("Model\t")
            || line.starts_with("Hardware\t")
            || line.starts_with("Processor\t")
        {
            if let Some((_, value)) = line.split_once(':') {
                let name = value.trim();
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
        }

        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let name = value.trim();
        if name.is_empty() {
            continue;
        }

        match key.trim() {
            "vendor_id" => vendor_id = name.to_string(),
            "cpu family" => family = name.to_string(),
            _ => {}
        }
    }

    let vendor_family = format!("{vendor_id} {family}").trim().to_string();
    if !vendor_family.is_empty() {
        return Some(vendor_family);
    }

    None
}

pub fn parse_lscpu_model_name(contents: &str) -> Option<String> {
    for line in contents.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.trim() != "Model name" {
            continue;
        }

        let name = value.trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }

    None
}

pub fn cpu_name_from_sources(
    lscpu_output: Option<&str>,
    sysinfo_brand: Option<&str>,
    cpuinfo_contents: Option<&str>,
) -> String {
    if let Some(name) = lscpu_output.and_then(parse_lscpu_model_name) {
        return name;
    }

    if let Some(name) = sysinfo_brand.map(str::trim).filter(|name| !name.is_empty()) {
        return name.to_string();
    }

    if let Some(name) = cpuinfo_contents.and_then(parse_cpuinfo_name) {
        return name;
    }

    "Unknown".to_string()
}

pub fn parse_os_release_pretty_name(contents: &str) -> Option<String> {
    for line in contents.lines() {
        let Some(value) = line.strip_prefix("PRETTY_NAME=") else {
            continue;
        };
        let pretty_name = value.trim().trim_matches('"').trim();
        if !pretty_name.is_empty() {
            return Some(pretty_name.to_string());
        }
    }
    None
}

pub fn proxmox_os_name_from_parts(
    pveversion_output: &str,
    os_release_contents: &str,
) -> Option<String> {
    let output = pveversion_output.trim();
    if output.is_empty() {
        return Some("Proxmox VE".to_string());
    }

    let mut version = String::new();
    for line in output.lines().map(str::trim) {
        let Some(version_part) = line.strip_prefix("pve-manager/") else {
            continue;
        };
        let before_hash = version_part
            .split_once('/')
            .map(|(value, _)| value)
            .unwrap_or(version_part);
        version = before_hash
            .split_once('~')
            .map(|(value, _)| value)
            .unwrap_or(before_hash)
            .to_string();
        break;
    }

    if version.is_empty() {
        return Some("Proxmox VE".to_string());
    }

    let codename = parse_os_release_value(os_release_contents, "VERSION_CODENAME");
    match codename {
        Some(codename) if !codename.is_empty() => {
            Some(format!("Proxmox VE {version} ({codename})"))
        }
        _ => Some(format!("Proxmox VE {version}")),
    }
}

pub fn parse_synology_os_name(contents: &str) -> Option<String> {
    let mut unique = String::new();
    let mut udc_check_state = String::new();

    for line in contents.lines().map(str::trim) {
        if let Some(value) = line.strip_prefix("unique=") {
            unique = value.trim_matches('"').to_string();
        } else if let Some(value) = line.strip_prefix("udc_check_state=") {
            udc_check_state = value.trim_matches('"').to_string();
        }
    }

    if unique.is_empty() || !unique.contains("synology_") {
        return None;
    }

    let parts = unique.split('_').collect::<Vec<_>>();
    if parts.len() < 3 {
        return None;
    }

    let model = parts.last()?.to_ascii_uppercase();
    if model.is_empty() {
        return None;
    }

    let mut name = format!("Synology {model} DSM");
    if !udc_check_state.is_empty() {
        name.push(' ');
        name.push_str(&udc_check_state);
    }
    Some(name)
}

pub fn fnos_os_name_from_markers(
    build_version_contents: Option<&str>,
    trim_dir_exists: bool,
) -> Option<String> {
    if let Some(contents) = build_version_contents {
        let version = contents.trim();
        if !version.is_empty() {
            return Some(format!("fnOS {version}"));
        }
    }

    if trim_dir_exists {
        return Some("fnOS".to_string());
    }

    None
}

pub fn android_os_name_from_build_prop(contents: &str) -> Option<String> {
    let Some(version) = parse_property_value(contents, "ro.build.version.release") else {
        return Some("Android".to_string());
    };
    let model = parse_property_value(contents, "ro.product.model").unwrap_or_default();
    let brand = parse_property_value(contents, "ro.product.brand").unwrap_or_default();
    Some(android_os_name_from_parts(&version, &model, &brand))
}

pub fn kernel_version_from_uname_output(output: Option<&str>) -> String {
    match output {
        Some(output) => output.trim().to_string(),
        None => "Unknown".to_string(),
    }
}

pub fn parse_ip_address_list(contents: &str) -> IpAddresses {
    let mut addresses = IpAddresses::default();

    for value in contents.split_whitespace() {
        let Ok(ip) = value.parse::<IpAddr>() else {
            continue;
        };
        if ip.is_loopback() {
            continue;
        }
        match ip {
            IpAddr::V4(ipv4) if addresses.ipv4.is_empty() => {
                addresses.ipv4 = ipv4.to_string();
            }
            IpAddr::V6(ipv6) if addresses.ipv6.is_empty() && !ipv6.is_unicast_link_local() => {
                addresses.ipv6 = ipv6.to_string();
            }
            _ => {}
        }
        if !addresses.ipv4.is_empty() && !addresses.ipv6.is_empty() {
            break;
        }
    }

    addresses
}

pub fn parse_ip_addr_show_output(contents: &str, filter: &NetworkFilter) -> IpAddresses {
    let mut addresses = IpAddresses::default();
    let mut current_interface = String::new();
    let mut current_interface_up = false;

    for line in contents.lines().map(str::trim) {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.is_empty() {
            continue;
        }

        if fields.len() >= 2 && fields[0].trim_end_matches(':').parse::<u32>().is_ok() {
            let name = fields[1]
                .trim_end_matches(':')
                .split('@')
                .next()
                .unwrap_or(fields[1].trim_end_matches(':'));

            if matches!(fields.get(2), Some(&"inet" | &"inet6")) && fields.len() >= 4 {
                if filter.should_include(name) {
                    record_ip_addr_show_address(&mut addresses, fields[2], fields[3]);
                }
            } else {
                current_interface = name.to_string();
                current_interface_up = ip_addr_interface_header_is_up(&fields);
            }

            if !addresses.ipv4.is_empty() && !addresses.ipv6.is_empty() {
                break;
            }
            continue;
        }

        if !matches!(fields[0], "inet" | "inet6") || fields.len() < 2 {
            continue;
        }
        if current_interface_up && filter.should_include(&current_interface) {
            record_ip_addr_show_address(&mut addresses, fields[0], fields[1]);
            if !addresses.ipv4.is_empty() && !addresses.ipv6.is_empty() {
                break;
            }
        }
    }

    addresses
}

fn ip_addr_interface_header_is_up(fields: &[&str]) -> bool {
    fields
        .get(2)
        .and_then(|flags| flags.strip_prefix('<')?.strip_suffix('>'))
        .map(|flags| flags.split(',').any(|flag| flag == "UP"))
        .unwrap_or(false)
}

fn record_ip_addr_show_address(addresses: &mut IpAddresses, family: &str, raw_addr: &str) {
    let raw_addr = raw_addr
        .split_once('/')
        .map(|(addr, _)| addr)
        .unwrap_or(raw_addr);
    let Ok(ip) = raw_addr.parse::<IpAddr>() else {
        return;
    };

    match (family, ip) {
        ("inet", IpAddr::V4(ipv4)) if addresses.ipv4.is_empty() && !ipv4.is_loopback() => {
            addresses.ipv4 = ipv4.to_string();
        }
        ("inet6", IpAddr::V6(ipv6))
            if addresses.ipv6.is_empty()
                && !ipv6.is_loopback()
                && !ipv6.is_unicast_link_local() =>
        {
            addresses.ipv6 = ipv6.to_string();
        }
        _ => {}
    }
}

pub fn parse_public_ipv4_response(contents: &str) -> Option<String> {
    find_go_ipv4_regex_match(contents).map(ToOwned::to_owned)
}

pub fn parse_public_ipv6_response(contents: &str) -> Option<String> {
    find_go_ipv6_regex_match(contents).map(ToOwned::to_owned)
}

pub fn detect_container_from_cgroup(contents: &str) -> Option<String> {
    for line in contents.lines().map(|line| line.to_ascii_lowercase()) {
        if line_has_container_id_after(
            &line,
            &[
                "/docker-",
                "/docker/",
                "/cri-containerd-",
                "/cri-containerd/",
            ],
        ) {
            return Some("docker".to_string());
        }
        if line_has_container_id_after(&line, &["/libpod-", "/libpod_", "/podman-", "/podman_"]) {
            return Some("podman".to_string());
        }
        if line_has_required_scoped_container_id_after(&line, "/crio-") {
            return Some("container".to_string());
        }
        if line_has_kubepods_uid(&line) {
            return Some("kubernetes".to_string());
        }
        if line
            .rsplit_once("/lxc/")
            .is_some_and(|(_, tail)| !tail.is_empty() && !tail.contains('/'))
        {
            return Some("lxc".to_string());
        }
    }
    None
}

fn line_has_container_id_after(line: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| {
        line.rsplit_once(marker)
            .map(|(_, tail)| strip_optional_scope(tail))
            .is_some_and(is_container_hex_id)
    })
}

fn line_has_required_scoped_container_id_after(line: &str, marker: &str) -> bool {
    line.rsplit_once(marker)
        .and_then(|(_, tail)| tail.strip_suffix(".scope"))
        .is_some_and(is_container_hex_id)
}

fn strip_optional_scope(value: &str) -> &str {
    value.strip_suffix(".scope").unwrap_or(value)
}

fn is_container_hex_id(value: &str) -> bool {
    (12..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn line_has_kubepods_uid(line: &str) -> bool {
    line.match_indices("/kubepods").any(|(idx, marker)| {
        let after = &line[idx + marker.len()..];
        matches!(after.as_bytes().first(), Some(b'/' | b'.')) && contains_uuid(after)
    })
}

fn contains_uuid(value: &str) -> bool {
    value
        .as_bytes()
        .windows(36)
        .any(|candidate| is_uuid_bytes(candidate))
}

fn is_uuid_bytes(value: &[u8]) -> bool {
    value.len() == 36
        && value.iter().enumerate().all(|(idx, byte)| match idx {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

pub fn detect_container_from_markers(
    has_dockerenv: bool,
    has_containerenv: bool,
    has_agent_container_marker: bool,
    cgroup_contents: Option<&str>,
) -> Option<String> {
    if has_dockerenv {
        return Some("docker".to_string());
    }

    if has_containerenv {
        return cgroup_contents
            .and_then(detect_container_from_cgroup)
            .or_else(|| Some("container".to_string()));
    }

    cgroup_contents
        .and_then(detect_container_from_cgroup)
        .or_else(|| has_agent_container_marker.then(|| "container".to_string()))
}

pub fn virtualization_from_cpuid_parts(has_hypervisor: bool, vendor: &str) -> String {
    if !has_hypervisor {
        return "none".to_string();
    }

    let vendor = vendor.trim_matches('\0').trim().to_ascii_lowercase();
    if vendor.is_empty() {
        return "virtualized".to_string();
    }

    const VENDOR_MAP: &[(&str, &[&str])] = &[
        ("kvm", &["kvm"]),
        ("microsoft", &["microsoft", "hyper-v", "msvm", "mshyperv"]),
        ("vmware", &["vmware"]),
        ("xen", &["xen"]),
        ("bhyve", &["bhyve"]),
        ("qemu", &["qemu"]),
        ("parallels", &["parallels"]),
        ("oracle", &["oracle", "virtualbox", "vbox"]),
        ("acrn", &["acrn"]),
    ];

    for (name, keys) in VENDOR_MAP {
        if keys
            .iter()
            .any(|key| vendor == *key || vendor.contains(key))
        {
            return (*name).to_string();
        }
    }

    vendor
}

pub fn parse_lspci_gpu_name(contents: &str) -> Option<String> {
    const PRIORITY_VENDORS: &[&str] = &[
        "nvidia",
        "amd",
        "radeon",
        "intel",
        "arc",
        "snap",
        "qualcomm",
        "snapdragon",
    ];

    let lines = contents.lines().collect::<Vec<_>>();
    for line in &lines {
        let lower = line.to_ascii_lowercase();
        if is_display_pci_line(&lower) {
            for vendor in PRIORITY_VENDORS {
                if !lower.contains(vendor) {
                    continue;
                }
                if let Some(name) =
                    extract_lspci_device_name(line).filter(|name| !is_excluded_gpu_name(name))
                {
                    return Some(name);
                }
            }
        }
    }

    for line in lines {
        let lower = line.to_ascii_lowercase();
        if is_display_pci_line(&lower) {
            if let Some(name) =
                extract_lspci_device_name(line).filter(|name| !is_excluded_gpu_name(name))
            {
                return Some(name);
            }
        }
    }

    None
}

pub fn sysfs_drm_gpu_name_from_driver(
    driver_name: &str,
    compatible: Option<&[u8]>,
) -> Option<String> {
    let driver_name = driver_name.trim();
    if driver_name.is_empty() || is_excluded_drm_driver(driver_name) {
        return None;
    }

    if let Some(compatible) = compatible {
        if let Some(model) = parse_soc_gpu_model(driver_name, compatible) {
            return Some(model);
        }
    }

    match driver_name {
        "vc4" | "vc4-drm" => Some("Broadcom VideoCore IV/VI (Raspberry Pi)".to_string()),
        "v3d" | "v3d-drm" => Some("Broadcom V3D (Raspberry Pi 4/5)".to_string()),
        "msm" | "msm_drm" => Some("Qualcomm Adreno (Unknown Model)".to_string()),
        "panfrost" => Some("ARM Mali (Panfrost)".to_string()),
        "lima" => Some("ARM Mali (Lima)".to_string()),
        "sun4i-drm" | "sunxi-drm" => Some("Allwinner Display Engine".to_string()),
        "tegra" => Some("NVIDIA Tegra".to_string()),
        "ast" => Some("ASPEED Technology, Inc. ASPEED Graphics Family".to_string()),
        "i915" | "i915-drm" => Some("Intel Integrated Graphics".to_string()),
        "mgag200" => Some("Matrox G200 Series".to_string()),
        other => Some(format!("Direct Render Manager {other}")),
    }
}

pub fn parse_soc_gpu_model(driver_name: &str, raw_bytes: &[u8]) -> Option<String> {
    let content = raw_bytes
        .iter()
        .map(|byte| if *byte == 0 { b' ' } else { *byte })
        .collect::<Vec<_>>();
    let lower = String::from_utf8_lossy(&content).to_ascii_lowercase();

    if driver_name == "msm" || lower.contains("adreno") {
        if let Some(model) = first_token_after_marker(&lower, "adreno-", true)
            .or_else(|| first_token_after_marker(&lower, "adreno_", true))
        {
            return Some(format!("Qualcomm Adreno {model}"));
        }
        return Some("Qualcomm Adreno".to_string());
    }

    if driver_name == "panfrost" || driver_name == "lima" || lower.contains("mali") {
        if let Some(model) = first_token_after_marker(&lower, "mali-", false)
            .or_else(|| first_token_after_marker(&lower, "mali_", false))
        {
            return Some(format!("ARM Mali {}", model.to_ascii_uppercase()));
        }
        return Some("ARM Mali".to_string());
    }

    if driver_name == "vc4" || driver_name == "vc4-drm" || driver_name == "v3d" {
        if lower.contains("bcm2712") {
            return Some("Broadcom VideoCore VII (Pi 5)".to_string());
        }
        if lower.contains("bcm2711") {
            return Some("Broadcom VideoCore VI (Pi 4)".to_string());
        }
        if lower.contains("bcm2837") || lower.contains("bcm2835") {
            return Some("Broadcom VideoCore IV".to_string());
        }
    }

    if lower.contains("allwinner") || lower.contains("sun50i") || lower.contains("sun8i") {
        if let Some(model) = allwinner_soc_model(&lower) {
            return Some(format!("Allwinner {model}"));
        }
        return Some("Allwinner Display Engine".to_string());
    }

    if driver_name == "tegra" {
        if lower.contains("tegra194") {
            return Some("NVIDIA Tegra Xavier".to_string());
        }
        if lower.contains("tegra234") {
            return Some("NVIDIA Orin".to_string());
        }
        if lower.contains("tegra210") {
            return Some("NVIDIA Tegra X1".to_string());
        }
    }

    None
}

pub fn parse_nvidia_smi_xml(contents: &str) -> Vec<GpuMetric> {
    nvidia_gpu_xml_chunks(contents)
        .into_iter()
        .map(|gpu| GpuMetric {
            name: xml_tag_text(gpu, "product_name").unwrap_or_default(),
            memory_total: parse_mib_value(
                &xml_child_tag_text(gpu, "fb_memory_usage", "total").unwrap_or_default(),
            ),
            memory_used: parse_mib_value(
                &xml_child_tag_text(gpu, "fb_memory_usage", "used").unwrap_or_default(),
            ),
            utilization: parse_percent_value(&xml_tag_text(gpu, "gpu_util").unwrap_or_default()),
            temperature: parse_temperature_value(
                &xml_tag_text(gpu, "gpu_temp").unwrap_or_default(),
            ),
        })
        .collect()
}

fn nvidia_gpu_xml_chunks(contents: &str) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut search_start = 0;
    while let Some(relative_start) = contents[search_start..].find("<gpu") {
        let tag_start = search_start + relative_start;
        let after_name = tag_start + "<gpu".len();
        let Some(next_byte) = contents.as_bytes().get(after_name) else {
            break;
        };
        if !matches!(next_byte, b'>' | b' ' | b'\t' | b'\r' | b'\n') {
            search_start = after_name;
            continue;
        }

        let Some(relative_tag_end) = contents[after_name..].find('>') else {
            break;
        };
        let body_start = after_name + relative_tag_end + 1;
        let Some(relative_body_end) = contents[body_start..].find("</gpu>") else {
            break;
        };
        let body_end = body_start + relative_body_end;
        chunks.push(&contents[body_start..body_end]);
        search_start = body_end + "</gpu>".len();
    }

    chunks
}

pub fn parse_amd_rocm_smi_json(contents: &str) -> Vec<GpuMetric> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(contents) else {
        return Vec::new();
    };
    let Some(cards) = value.as_object() else {
        return Vec::new();
    };

    let mut keys = cards
        .keys()
        .filter(|key| key.starts_with("card"))
        .cloned()
        .collect::<Vec<_>>();
    keys.sort();

    keys.into_iter()
        .filter_map(|key| cards.get(&key)?.as_object())
        .map(|card| GpuMetric {
            name: json_string(card, "Card series"),
            memory_total: parse_unsigned_i64_value(&json_string(card, "VRAM Total Memory (B)")),
            memory_used: parse_unsigned_i64_value(&json_string(card, "VRAM Total Used Memory (B)")),
            utilization: parse_percent_value(&json_string(card, "GPU use (%)")),
            temperature: parse_temperature_value(&json_string(
                card,
                "Temperature (Sensor junction) (C)",
            )),
        })
        .collect()
}

pub fn parse_meminfo(contents: &str) -> ProcMemInfo {
    let mut info = ProcMemInfo::default();

    for line in contents.lines() {
        let mut fields = line.split_whitespace();
        let Some(key) = fields.next() else {
            continue;
        };
        let Some(value) = fields.next().and_then(parse_meminfo_kib_value) else {
            continue;
        };
        let bytes = value.saturating_mul(1024);

        match key.trim_end_matches(':') {
            "MemTotal" => info.mem_total = bytes,
            "MemFree" => info.mem_free = bytes,
            "MemAvailable" => info.mem_available = bytes,
            "Buffers" => info.buffers = bytes,
            "Cached" => info.cached = bytes,
            "SwapTotal" => info.swap_total = bytes,
            "SwapFree" => info.swap_free = bytes,
            "SwapCached" => info.swap_cached = bytes,
            "Shmem" => info.shmem = bytes,
            "SReclaimable" => info.s_reclaimable = bytes,
            "Zswap" => info.zswap = bytes,
            "Zswapped" => info.zswapped = bytes,
            _ => {}
        }
    }

    info
}

fn parse_meminfo_kib_value(value: &str) -> Option<i64> {
    value
        .parse::<u64>()
        .ok()
        .and_then(|value| i64::try_from(value).ok())
}

pub fn go_compatible_ram(info: &ProcMemInfo) -> MemoryValues {
    let used = go_compatible_ram_base_used(info)
        .saturating_add(info.shmem)
        .max(0);

    MemoryValues {
        total: info.mem_total,
        used,
    }
}

pub fn go_compatible_ram_raw_used(info: &ProcMemInfo) -> MemoryValues {
    if info.mem_total <= 0 {
        return MemoryValues::default();
    }

    go_compatible_ram(info)
}

fn go_compatible_ram_base_used(info: &ProcMemInfo) -> i64 {
    let free_like = info.mem_free + info.cached + info.s_reclaimable + info.buffers;
    if info.mem_total >= free_like {
        info.mem_total - free_like
    } else {
        info.mem_total.saturating_sub(info.mem_free)
    }
}

pub fn go_compatible_ram_include_cache(info: &ProcMemInfo) -> MemoryValues {
    MemoryValues {
        total: info.mem_total,
        used: info.mem_total.saturating_sub(info.mem_free).max(0),
    }
}

pub fn go_compatible_swap(info: &ProcMemInfo) -> MemoryValues {
    let deductions = info.swap_free + info.swap_cached;
    let used = if info.swap_total >= deductions {
        info.swap_total - deductions
    } else {
        info.swap_total.saturating_sub(info.swap_free)
    };

    MemoryValues {
        total: info.swap_total,
        used: used.max(0),
    }
}

pub fn memory_values_from_meminfo_with_modes(
    info: &ProcMemInfo,
    include_cache: bool,
    report_raw_used: bool,
) -> Option<(MemoryValues, MemoryValues)> {
    let selection = memory_selection_from_meminfo_with_modes(info, include_cache, report_raw_used);
    selection.ram.map(|ram| (ram, selection.swap))
}

pub fn memory_selection_from_meminfo_with_modes(
    info: &ProcMemInfo,
    include_cache: bool,
    report_raw_used: bool,
) -> MemorySelection {
    let ram = if report_raw_used {
        Some(go_compatible_ram_raw_used(info))
    } else if info.mem_total <= 0 {
        None
    } else if include_cache {
        Some(go_compatible_ram_include_cache(info))
    } else {
        Some(go_compatible_ram(info))
    };

    MemorySelection {
        ram,
        swap: go_compatible_swap(info),
    }
}

pub fn go_compatible_disk(mounts: &[DiskMount]) -> DiskValues {
    let mut devices = HashMap::<String, DiskValues>::new();

    for mount in mounts {
        if !is_physical_disk(mount) {
            continue;
        }

        let device_id = disk_device_id(mount);
        let values = DiskValues {
            total: mount.total.max(0),
            used: mount.used.clamp(0, mount.total.max(0)),
        };
        match devices.get(&device_id) {
            Some(existing) if existing.total >= values.total => {}
            _ => {
                devices.insert(device_id, values);
            }
        }
    }

    devices
        .values()
        .fold(DiskValues::default(), |mut total, values| {
            total.total += values.total;
            total.used += values.used;
            total
        })
}

pub fn go_compatible_disk_with_mountpoints(
    mounts: &[DiskMount],
    include_mountpoints: &str,
) -> DiskValues {
    let include = parse_semicolon_list(include_mountpoints);
    if include.is_empty() {
        return go_compatible_disk(mounts);
    }

    mounts
        .iter()
        .filter(|mount| include.iter().any(|item| item == &mount.mountpoint))
        .fold(DiskValues::default(), |mut total, mount| {
            total.total += mount.total.max(0);
            total.used += mount.used.clamp(0, mount.total.max(0));
            total
        })
}

pub fn go_compatible_disk_from_mountpoint_lookup<F>(
    include_mountpoints: &str,
    mut lookup: F,
) -> DiskValues
where
    F: FnMut(&str) -> Option<DiskValues>,
{
    parse_semicolon_list(include_mountpoints)
        .into_iter()
        .filter_map(|mountpoint| lookup(&mountpoint))
        .fold(DiskValues::default(), |mut total, values| {
            total.total += values.total.max(0);
            total.used += values.used.clamp(0, values.total.max(0));
            total
        })
}

pub fn collect_disk_values_with_mountpoints(
    mounts: &[DiskMount],
    include_mountpoints: &str,
) -> DiskValues {
    if parse_semicolon_list(include_mountpoints).is_empty() {
        return go_compatible_disk(mounts);
    }

    go_compatible_disk_from_mountpoint_lookup(include_mountpoints, disk_usage_for_mountpoint)
}

pub fn parse_net_dev(contents: &str) -> NetworkTotals {
    parse_net_dev_with_filter(contents, &NetworkFilter::default())
}

pub fn parse_net_dev_with_filter(contents: &str, filter: &NetworkFilter) -> NetworkTotals {
    parse_net_dev_interfaces(contents, filter).into_iter().fold(
        NetworkTotals::default(),
        |mut totals, interface| {
            totals.total_down += interface.total_down;
            totals.total_up += interface.total_up;
            totals
        },
    )
}

pub fn network_speed_from_samples(
    first: NetworkTotals,
    second: NetworkTotals,
    elapsed_seconds: f64,
) -> NetworkSpeedSample {
    let elapsed = elapsed_seconds.max(1.0);
    NetworkSpeedSample {
        total: second,
        speed: NetworkTotals {
            total_up: ((second.total_up - first.total_up).max(0) as f64 / elapsed) as i64,
            total_down: ((second.total_down - first.total_down).max(0) as f64 / elapsed) as i64,
        },
    }
}

pub fn parse_net_dev_interfaces(
    contents: &str,
    filter: &NetworkFilter,
) -> Vec<NetworkInterfaceTotals> {
    let mut interfaces = Vec::new();
    for line in contents.lines() {
        let Some((name, counters)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if !filter.should_include(name) {
            continue;
        }

        let fields = counters.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 16 {
            continue;
        }

        let rx_bytes = fields[0].parse::<i64>().unwrap_or(0);
        let tx_bytes = fields[8].parse::<i64>().unwrap_or(0);
        interfaces.push(NetworkInterfaceTotals {
            name: name.to_string(),
            total_up: tx_bytes,
            total_down: rx_bytes,
        });
    }

    interfaces
}

pub fn parse_net_static_total_between(
    contents: &str,
    start: u64,
    end: u64,
    filter: &NetworkFilter,
) -> Option<NetworkTotals> {
    let value = serde_json::from_str::<serde_json::Value>(contents).ok()?;
    let interfaces = value.get("interfaces")?.as_object()?;
    let mut totals = NetworkTotals::default();

    for (name, entries) in interfaces {
        if !filter.should_include(name) {
            continue;
        }
        let Some(entries) = entries.as_array() else {
            continue;
        };
        for entry in entries {
            let timestamp = entry
                .get("timestamp")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            if (start != 0 && timestamp < start) || (end != 0 && timestamp > end) {
                continue;
            }
            totals.total_up += entry
                .get("tx")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            totals.total_down += entry
                .get("rx")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
        }
    }

    Some(totals)
}

pub fn reset_date_ymd(reset_day: u32, year: i32, month: u32, day: u32) -> (i32, u32, u32) {
    if !(1..=31).contains(&reset_day) {
        return (year, month, day);
    }

    let current_reset = actual_reset_date(year, month, reset_day);
    if (year, month, day) >= current_reset {
        return current_reset;
    }

    let (previous_year, previous_month) = if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    };
    actual_reset_date(previous_year, previous_month, reset_day)
}

pub fn count_socket_entries(contents: &str) -> i32 {
    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("sl "))
        .filter(|line| {
            line.split_whitespace()
                .next()
                .is_some_and(|field| field.ends_with(':'))
        })
        .count() as i32
}

pub fn collect_proc_metrics() -> Option<ProcMetrics> {
    collect_proc_metrics_with_filter(&NetworkFilter::default())
}

pub fn collect_proc_metrics_with_filter(filter: &NetworkFilter) -> Option<ProcMetrics> {
    collect_proc_metrics_with_filter_and_host_proc(filter, "")
}

pub fn collect_proc_metrics_with_filter_and_host_proc(
    filter: &NetworkFilter,
    host_proc: &str,
) -> Option<ProcMetrics> {
    collect_proc_metrics_sample_with_filter_and_host_proc(filter, host_proc)
        .map(|sample| sample.metrics)
}

pub fn collect_proc_metrics_sample_with_filter_and_host_proc(
    filter: &NetworkFilter,
    host_proc: &str,
) -> Option<ProcMetricsSample> {
    if !linux_supported() {
        return None;
    }

    let proc_root = proc_root_for(host_proc);
    Some(collect_proc_metrics_sample_with_filter_and_proc_root(
        proc_root, filter,
    ))
}

pub fn collect_proc_stat_cpu_sample_with_host_proc(host_proc: &str) -> Option<ProcStatCpuSample> {
    if !linux_supported() {
        return None;
    }

    read_proc_stat_cpu_sample_with_proc_root(proc_root_for(host_proc))
}

pub fn read_proc_stat_cpu_sample_with_proc_root<P: AsRef<Path>>(
    proc_root: P,
) -> Option<ProcStatCpuSample> {
    let contents = fs::read_to_string(proc_root.as_ref().join("stat")).ok()?;
    parse_proc_stat_cpu_sample(&contents)
}

pub fn collect_proc_metrics_sample_with_filter_and_proc_root<P: AsRef<Path>>(
    proc_root: P,
    filter: &NetworkFilter,
) -> ProcMetricsSample {
    let proc_root = proc_root.as_ref();
    let mut errors = ProcMetricErrors::default();

    let load = fs::read_to_string(proc_root.join("loadavg"))
        .ok()
        .and_then(|contents| parse_loadavg(&contents))
        .unwrap_or_default();

    let uptime = match fs::read_to_string(proc_root.join("uptime")) {
        Ok(contents) => parse_uptime(&contents).unwrap_or_else(|| {
            errors.uptime = Some("/proc/uptime is not readable or invalid".to_string());
            0
        }),
        Err(_) => {
            errors.uptime = Some("/proc/uptime is not readable or invalid".to_string());
            0
        }
    };

    let network = match collect_network_totals_with_filter_and_proc_root(proc_root, filter) {
        Ok(network) => network,
        Err(_) => {
            errors.network_speed = Some(
                "failed to get network IO counters: /proc/net/dev is not readable".to_string(),
            );
            NetworkTotals::default()
        }
    };

    let (tcp_connections, udp_connections) = match count_tcp_udp_sockets_in_proc_root(proc_root) {
        Ok((tcp, udp)) => (tcp, udp),
        Err(error) => {
            errors.connections = Some(error);
            (0, 0)
        }
    };
    let process_count = count_process_entries_in_dir(proc_root);

    ProcMetricsSample {
        metrics: proc_metrics_from_parts(
            load,
            uptime,
            network,
            tcp_connections,
            udp_connections,
            process_count,
        ),
        errors,
    }
}

fn proc_root_for(path: &str) -> std::path::PathBuf {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Path::new("/proc").to_path_buf();
    }

    let candidate = Path::new(trimmed);
    if candidate.is_dir() {
        candidate.to_path_buf()
    } else {
        Path::new("/proc").to_path_buf()
    }
}

pub fn collect_network_interfaces_with_filter(
    filter: &NetworkFilter,
) -> Option<Vec<NetworkInterfaceTotals>> {
    if !linux_supported() {
        return None;
    }

    fs::read_to_string("/proc/net/dev")
        .ok()
        .map(|contents| parse_net_dev_interfaces(&contents, filter))
}

pub fn collect_network_totals_with_filter_and_host_proc(
    filter: &NetworkFilter,
    host_proc: &str,
) -> Option<Result<NetworkTotals, String>> {
    if !linux_supported() {
        return None;
    }

    Some(collect_network_totals_with_filter_and_proc_root(
        proc_root_for(host_proc),
        filter,
    ))
}

pub fn collect_network_totals_with_filter_and_proc_root<P: AsRef<Path>>(
    proc_root: P,
    filter: &NetworkFilter,
) -> Result<NetworkTotals, String> {
    fs::read_to_string(proc_root.as_ref().join("net").join("dev"))
        .map(|contents| parse_net_dev_with_filter(&contents, filter))
        .map_err(|_| "failed to get network IO counters: /proc/net/dev is not readable".to_string())
}

pub fn collect_net_static_total_between<P: AsRef<Path>>(
    path: P,
    start: u64,
    end: u64,
    filter: &NetworkFilter,
) -> Option<NetworkTotals> {
    let contents = fs::read_to_string(path).ok()?;
    parse_net_static_total_between(&contents, start, end, filter)
}

pub fn reset_timestamp_for_day(reset_day: u32, now: DateTime<Local>) -> Option<u64> {
    let (year, month, day) = reset_date_ymd(reset_day, now.year(), now.month(), now.day());
    Local
        .with_ymd_and_hms(year, month, day, 0, 0, 0)
        .single()
        .map(|reset| reset.timestamp().max(0) as u64)
}

pub fn collect_current_month_net_static_totals<P: AsRef<Path>>(
    path: P,
    reset_day: u32,
    filter: &NetworkFilter,
) -> Option<NetworkTotals> {
    let now = Local::now();
    let start = reset_timestamp_for_day(reset_day, now)?;
    let end = now.timestamp().max(0) as u64;
    collect_net_static_total_between(path, start, end, filter)
}

pub fn collect_memory_values() -> Option<(MemoryValues, MemoryValues)> {
    collect_memory_values_with_mode(false)
}

pub fn collect_memory_values_with_mode(
    include_cache: bool,
) -> Option<(MemoryValues, MemoryValues)> {
    collect_memory_values_with_modes(include_cache, false)
}

pub fn collect_memory_values_with_modes(
    include_cache: bool,
    report_raw_used: bool,
) -> Option<(MemoryValues, MemoryValues)> {
    collect_memory_selection_with_modes(include_cache, report_raw_used)
        .and_then(|selection| selection.ram.map(|ram| (ram, selection.swap)))
}

pub fn collect_memory_selection_with_modes(
    include_cache: bool,
    report_raw_used: bool,
) -> Option<MemorySelection> {
    if !linux_supported() {
        return None;
    }

    let meminfo = fs::read_to_string("/proc/meminfo")
        .ok()
        .map(|contents| parse_meminfo(&contents))?;
    Some(memory_selection_from_meminfo_with_modes(
        &meminfo,
        include_cache,
        report_raw_used,
    ))
}

pub fn collect_cpuinfo_name() -> Option<String> {
    if !linux_supported() {
        return None;
    }

    fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|contents| parse_cpuinfo_name(&contents))
}

pub fn collect_lscpu_model_name() -> Option<String> {
    if !linux_supported() {
        return None;
    }

    let output = Command::new("lscpu").output().ok()?;
    if !output.status.success() {
        return None;
    }

    parse_lscpu_model_name(&String::from_utf8_lossy(&output.stdout))
}

pub fn collect_cpu_name(sysinfo_brand: Option<&str>) -> String {
    if let Some(name) = collect_lscpu_model_name() {
        return name;
    }

    let cpuinfo_contents = if linux_supported() {
        fs::read_to_string("/proc/cpuinfo").ok()
    } else {
        None
    };

    cpu_name_from_sources(None, sysinfo_brand, cpuinfo_contents.as_deref())
}

pub fn collect_os_name() -> String {
    if !linux_supported() {
        return std::env::consts::OS.to_string();
    }

    if let Some(name) = collect_android_os_name() {
        return name;
    }
    if let Some(name) = collect_proxmox_os_name() {
        return name;
    }
    if let Some(name) = collect_synology_os_name() {
        return name;
    }
    if let Some(name) = collect_fnos_os_name() {
        return name;
    }

    fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|contents| parse_os_release_pretty_name(&contents))
        .unwrap_or_else(|| "Linux".to_string())
}

pub fn collect_kernel_version() -> String {
    if !linux_supported() {
        return std::env::consts::OS.to_string();
    }

    if let Some(output) = Command::new("uname")
        .arg("-r")
        .output()
        .ok()
        .filter(|output| output.status.success())
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        return kernel_version_from_uname_output(Some(&stdout));
    }

    kernel_version_from_uname_output(None)
}

fn collect_android_os_name() -> Option<String> {
    if let Ok(output) = Command::new("getprop")
        .arg("ro.build.version.release")
        .output()
    {
        if output.status.success() {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !version.is_empty() {
                let model = getprop_value("ro.product.model");
                let brand = getprop_value("ro.product.brand");
                return Some(android_os_name_from_parts(&version, &model, &brand));
            }
        }
    }

    if Path::new("/system/build.prop").is_file() {
        if let Ok(contents) = fs::read_to_string("/system/build.prop") {
            return android_os_name_from_build_prop(&contents)
                .or_else(|| Some("Android".to_string()));
        }
    }

    let android_dirs = ["/system/app", "/system/priv-app", "/data/app", "/sdcard"];
    let dir_count = android_dirs
        .iter()
        .filter(|path| Path::new(path).is_dir())
        .count();
    if dir_count >= 2 {
        return Some("Android".to_string());
    }

    None
}

fn collect_proxmox_os_name() -> Option<String> {
    let output = Command::new("pveversion").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let os_release = fs::read_to_string("/etc/os-release").unwrap_or_default();
    proxmox_os_name_from_parts(&String::from_utf8_lossy(&output.stdout), &os_release)
}

fn collect_synology_os_name() -> Option<String> {
    for path in ["/etc/synoinfo.conf", "/etc.defaults/synoinfo.conf"] {
        if Path::new(path).is_file() {
            if let Ok(contents) = fs::read_to_string(path) {
                if let Some(name) = parse_synology_os_name(&contents) {
                    return Some(name);
                }
            }
        }
    }

    if Path::new("/usr/syno").is_dir() {
        return Some("Synology DSM".to_string());
    }
    None
}

fn collect_fnos_os_name() -> Option<String> {
    let build_version = if Path::new("/usr/trim/BUILD_VERSION").is_file() {
        fs::read_to_string("/usr/trim/BUILD_VERSION").ok()
    } else {
        None
    };
    fnos_os_name_from_markers(build_version.as_deref(), Path::new("/usr/trim").is_dir())
}

pub fn collect_ip_addresses() -> Option<IpAddresses> {
    collect_ip_addresses_with_filter(&NetworkFilter::default())
}

pub fn collect_ip_addresses_with_filter(filter: &NetworkFilter) -> Option<IpAddresses> {
    if !linux_supported() {
        return None;
    }

    if let Ok(output) = Command::new("ip")
        .args(["-o", "addr", "show", "up"])
        .output()
    {
        if output.status.success() {
            let addresses =
                parse_ip_addr_show_output(&String::from_utf8_lossy(&output.stdout), filter);
            if !addresses.ipv4.is_empty() || !addresses.ipv6.is_empty() {
                return Some(addresses);
            }
        }
    }

    let output = Command::new("hostname").arg("-I").output().ok()?;
    if !output.status.success() {
        return None;
    }

    Some(parse_ip_address_list(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

pub fn collect_public_ip_addresses() -> IpAddresses {
    collect_public_ip_addresses_with_dns("")
}

pub fn collect_public_ip_addresses_with_dns(custom_dns: &str) -> IpAddresses {
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("curl/8.0.1");

    let custom_dns = custom_dns.trim();
    if !custom_dns.is_empty() {
        builder = builder.dns_resolver(Arc::new(CustomDnsResolver::new(custom_dns)));
    }

    let client = match builder.build() {
        Ok(client) => client,
        Err(_) => return IpAddresses::default(),
    };

    IpAddresses {
        ipv4: probe_public_ipv4(&client).unwrap_or_default(),
        ipv6: probe_public_ipv6(&client).unwrap_or_default(),
    }
}

#[derive(Debug)]
pub struct CustomDnsResolver {
    server: String,
    timeout: Duration,
}

impl CustomDnsResolver {
    pub fn new(server: &str) -> Self {
        Self {
            server: normalize_dns_server(server),
            timeout: Duration::from_secs(10),
        }
    }
}

impl reqwest::dns::Resolve for CustomDnsResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let result = resolve_host_with_dns_server(&self.server, name.as_str(), self.timeout)
            .map(|addrs| Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
            .map_err(|error| Box::new(error) as Box<dyn Error + Send + Sync>);
        Box::pin(std::future::ready(result))
    }
}

pub fn normalize_dns_server(server: &str) -> String {
    let server = server.trim();
    if (server.starts_with('[') && server.contains("]:"))
        || (server.matches(':').count() == 1 && !server.contains(']'))
    {
        return server.to_string();
    }
    if server.matches(':').count() >= 2 && !server.contains(']') {
        return format!("[{server}]:53");
    }
    if !server.contains(':') {
        return format!("{server}:53");
    }
    server.to_string()
}

pub fn resolve_host_with_dns_server(
    dns_server: &str,
    host: &str,
    timeout: Duration,
) -> std::io::Result<Vec<SocketAddr>> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, 0)]);
    }

    let dns_server = normalize_dns_server(dns_server);
    let server_addr = dns_server
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "empty DNS server"))?;

    let mut addrs = Vec::new();
    addrs.extend(query_dns_records(server_addr, host, 1, timeout)?);
    addrs.extend(query_dns_records(server_addr, host, 28, timeout)?);

    if addrs.is_empty() {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "DNS response contained no address records",
        ))
    } else {
        Ok(addrs)
    }
}

fn query_dns_records(
    server_addr: SocketAddr,
    host: &str,
    qtype: u16,
    timeout: Duration,
) -> std::io::Result<Vec<SocketAddr>> {
    let query_id = dns_query_id(host, qtype);
    let query = build_dns_query(query_id, host, qtype)?;
    let bind_addr = if server_addr.is_ipv6() {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    };
    let socket = UdpSocket::bind(bind_addr)?;
    socket.set_read_timeout(Some(timeout))?;
    socket.set_write_timeout(Some(timeout))?;
    socket.send_to(&query, server_addr)?;

    let mut response = [0_u8; 1232];
    let (len, _) = socket.recv_from(&mut response)?;
    parse_dns_response(&response[..len], query_id, qtype)
}

fn dns_query_id(host: &str, qtype: u16) -> u16 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos())
        .unwrap_or(0);
    host.bytes().fold((nanos as u16) ^ qtype, |acc, byte| {
        acc.wrapping_mul(31) ^ byte as u16
    })
}

fn build_dns_query(id: u16, host: &str, qtype: u16) -> std::io::Result<Vec<u8>> {
    let mut packet = Vec::with_capacity(512);
    packet.extend_from_slice(&id.to_be_bytes());
    packet.extend_from_slice(&0x0100_u16.to_be_bytes());
    packet.extend_from_slice(&1_u16.to_be_bytes());
    packet.extend_from_slice(&0_u16.to_be_bytes());
    packet.extend_from_slice(&0_u16.to_be_bytes());
    packet.extend_from_slice(&0_u16.to_be_bytes());

    for label in host.trim_end_matches('.').split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid DNS name",
            ));
        }
        packet.push(label.len() as u8);
        packet.extend_from_slice(label.as_bytes());
    }
    packet.push(0);
    packet.extend_from_slice(&qtype.to_be_bytes());
    packet.extend_from_slice(&1_u16.to_be_bytes());
    Ok(packet)
}

fn parse_dns_response(
    packet: &[u8],
    expected_id: u16,
    qtype: u16,
) -> std::io::Result<Vec<SocketAddr>> {
    if packet.len() < 12 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "short DNS response",
        ));
    }
    let response_id = u16::from_be_bytes([packet[0], packet[1]]);
    if response_id != expected_id {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "mismatched DNS response id",
        ));
    }

    let flags = u16::from_be_bytes([packet[2], packet[3]]);
    if flags & 0x000f != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "DNS response returned an error",
        ));
    }

    let questions = u16::from_be_bytes([packet[4], packet[5]]) as usize;
    let answers = u16::from_be_bytes([packet[6], packet[7]]) as usize;
    let mut offset = 12;
    for _ in 0..questions {
        offset = skip_dns_name(packet, offset)?;
        offset = offset.checked_add(4).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "DNS question overflow")
        })?;
        if offset > packet.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "truncated DNS question",
            ));
        }
    }

    let mut addrs = Vec::new();
    for _ in 0..answers {
        offset = skip_dns_name(packet, offset)?;
        if offset + 10 > packet.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "truncated DNS answer",
            ));
        }
        let answer_type = u16::from_be_bytes([packet[offset], packet[offset + 1]]);
        let answer_class = u16::from_be_bytes([packet[offset + 2], packet[offset + 3]]);
        let rdlen = u16::from_be_bytes([packet[offset + 8], packet[offset + 9]]) as usize;
        offset += 10;
        if offset + rdlen > packet.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "truncated DNS rdata",
            ));
        }

        let rdata = &packet[offset..offset + rdlen];
        if answer_class == 1 && answer_type == qtype {
            match (answer_type, rdlen) {
                (1, 4) => addrs.push(SocketAddr::new(
                    IpAddr::from([rdata[0], rdata[1], rdata[2], rdata[3]]),
                    0,
                )),
                (28, 16) => {
                    let mut bytes = [0_u8; 16];
                    bytes.copy_from_slice(rdata);
                    addrs.push(SocketAddr::new(IpAddr::from(bytes), 0));
                }
                _ => {}
            }
        }
        offset += rdlen;
    }

    Ok(addrs)
}

fn skip_dns_name(packet: &[u8], mut offset: usize) -> std::io::Result<usize> {
    loop {
        let Some(&len) = packet.get(offset) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "truncated DNS name",
            ));
        };
        if len & 0xC0 == 0xC0 {
            if packet.get(offset + 1).is_none() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "truncated DNS pointer",
                ));
            }
            return Ok(offset + 2);
        }
        if len == 0 {
            return Ok(offset + 1);
        }
        offset = offset.checked_add(1 + len as usize).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "DNS name overflow")
        })?;
        if offset > packet.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "truncated DNS label",
            ));
        }
    }
}

pub fn detect_virtualization() -> String {
    if !linux_supported() {
        return "none".to_string();
    }

    if let Ok(output) = Command::new("systemd-detect-virt").output() {
        if output.status.success() {
            let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !value.is_empty() {
                return value;
            }
        }
    }

    let cgroup_contents = fs::read_to_string("/proc/self/cgroup").ok();
    if let Some(container) = detect_container_from_markers(
        fs::metadata("/.dockerenv").is_ok(),
        fs::metadata("/run/.containerenv").is_ok(),
        fs::metadata("/.kelicloud-agent-container").is_ok()
            || fs::metadata("/.komari-agent-container").is_ok(),
        cgroup_contents.as_deref(),
    ) {
        return container;
    }

    detect_by_cpuid()
}

pub fn collect_gpu_name() -> String {
    if !linux_supported() {
        return "None".to_string();
    }

    if let Some(name) = Command::new("lspci")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| parse_lspci_gpu_name(&String::from_utf8_lossy(&output.stdout)))
    {
        return name;
    }

    collect_sysfs_drm_gpu_name().unwrap_or_else(|| "None".to_string())
}

pub fn collect_detailed_gpu_metrics_result() -> Result<Vec<GpuMetric>, String> {
    if !linux_supported() {
        return Err("detailed GPU monitoring not supported on this platform".to_string());
    }

    let mut errors = Vec::new();

    if let Ok(output) = Command::new("nvidia-smi").args(["-q", "-x"]).output() {
        if output.status.success() {
            let metrics = parse_nvidia_smi_xml(&String::from_utf8_lossy(&output.stdout));
            if !metrics.is_empty() {
                return Ok(metrics);
            }
            errors.push("nvidia-smi returned no GPU metrics".to_string());
        } else {
            errors.push(command_output_error("nvidia-smi", &output));
        }
    } else {
        errors.push("nvidia-smi not found".to_string());
    }

    if let Ok(output) = Command::new("rocm-smi")
        .args(["--showallinfo", "--json"])
        .output()
    {
        if output.status.success() {
            let metrics = parse_amd_rocm_smi_json(&String::from_utf8_lossy(&output.stdout));
            if !metrics.is_empty() {
                return Ok(metrics);
            }
            errors.push("rocm-smi returned no GPU metrics".to_string());
        } else {
            errors.push(command_output_error("rocm-smi", &output));
        }
    } else {
        errors.push("rocm-smi not found".to_string());
    }

    Err(errors.join("; "))
}

pub fn collect_detailed_gpu_metrics() -> Vec<GpuMetric> {
    collect_detailed_gpu_metrics_result().unwrap_or_default()
}

pub fn collect_detailed_gpu_models_result() -> Result<Vec<String>, String> {
    if !linux_supported() {
        return Err("detailed GPU monitoring not supported on this platform".to_string());
    }

    let mut errors = Vec::new();

    if let Ok(output) = Command::new("nvidia-smi").args(["-q", "-x"]).output() {
        if output.status.success() {
            let models = parse_nvidia_smi_xml(&String::from_utf8_lossy(&output.stdout))
                .into_iter()
                .map(|metric| metric.name.trim().to_string())
                .filter(|name| !name.is_empty())
                .collect::<Vec<_>>();
            if !models.is_empty() {
                return Ok(models);
            }
            errors.push("nvidia-smi returned no GPU models".to_string());
        } else {
            errors.push(command_output_error("nvidia-smi", &output));
        }
    } else {
        errors.push("nvidia-smi not found".to_string());
    }

    if let Ok(output) = Command::new("rocm-smi")
        .args(["--showallinfo", "--json"])
        .output()
    {
        if output.status.success() {
            let models = parse_amd_rocm_smi_json(&String::from_utf8_lossy(&output.stdout))
                .into_iter()
                .map(|metric| metric.name.trim().to_string())
                .filter(|name| !name.is_empty())
                .collect::<Vec<_>>();
            if !models.is_empty() {
                return Ok(models);
            }
            errors.push("rocm-smi returned no GPU models".to_string());
        } else {
            errors.push(command_output_error("rocm-smi", &output));
        }
    } else {
        errors.push("rocm-smi not found".to_string());
    }

    Err(errors.join("; "))
}

fn command_output_error(command: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        format!("{command} exited with {}", output.status)
    } else {
        format!("{command}: {stderr}")
    }
}

fn probe_public_ipv4(client: &reqwest::blocking::Client) -> Option<String> {
    const APIS: &[&str] = &[
        "https://www.visa.cn/cdn-cgi/trace",
        "https://www.qualcomm.cn/cdn-cgi/trace",
        "https://www.toutiao.com/stream/widget/local_weather/data/",
        "https://edge-ip.html.zone/geo",
        "https://vercel-ip.html.zone/geo",
        "http://ipv4.ip.sb",
        "https://api.ipify.org?format=json",
    ];
    for api in APIS {
        let Ok(body) = client.get(*api).send().and_then(|response| response.text()) else {
            continue;
        };
        if let Some(ip) = parse_public_ipv4_response(&body) {
            return Some(ip);
        }
    }
    None
}

fn probe_public_ipv6(client: &reqwest::blocking::Client) -> Option<String> {
    const APIS: &[&str] = &[
        "https://v6.ip.zxinc.org/info.php?type=json",
        "https://api6.ipify.org?format=json",
        "https://ipv6.icanhazip.com",
        "https://api-ipv6.ip.sb/geoip",
    ];
    for api in APIS {
        let Ok(body) = client.get(*api).send().and_then(|response| response.text()) else {
            continue;
        };
        if let Some(ip) = parse_public_ipv6_response(&body) {
            return Some(ip);
        }
    }
    None
}

fn parse_os_release_value(contents: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    contents.lines().find_map(|line| {
        line.strip_prefix(&prefix)
            .map(|value| value.trim().trim_matches('"').to_string())
            .filter(|value| !value.is_empty())
    })
}

fn parse_property_value(contents: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    contents.lines().find_map(|line| {
        line.trim()
            .strip_prefix(&prefix)
            .map(str::trim)
            .map(ToString::to_string)
            .filter(|value| !value.is_empty())
    })
}

fn android_os_name_from_parts(version: &str, model: &str, brand: &str) -> String {
    let mut result = format!("Android {}", version.trim());
    let model = model.trim();
    let brand = brand.trim();

    if !model.is_empty() {
        if !brand.is_empty() && brand != model {
            result.push_str(&format!(" ({brand} {model})"));
        } else {
            result.push_str(&format!(" ({model})"));
        }
    }

    result
}

fn getprop_value(key: &str) -> String {
    Command::new("getprop")
        .arg(key)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default()
}

#[cfg(target_arch = "x86")]
fn detect_by_cpuid() -> String {
    use std::arch::x86::{__cpuid, __cpuid_count};
    detect_by_cpuid_impl(
        || __cpuid(1).ecx & (1 << 31) != 0,
        || {
            let result = __cpuid_count(0x4000_0000, 0);
            cpuid_vendor_string(result.ebx, result.ecx, result.edx)
        },
    )
}

#[cfg(target_arch = "x86_64")]
fn detect_by_cpuid() -> String {
    use std::arch::x86_64::{__cpuid, __cpuid_count};
    detect_by_cpuid_impl(
        || __cpuid(1).ecx & (1 << 31) != 0,
        || {
            let result = __cpuid_count(0x4000_0000, 0);
            cpuid_vendor_string(result.ebx, result.ecx, result.edx)
        },
    )
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
fn detect_by_cpuid() -> String {
    "none".to_string()
}

fn detect_by_cpuid_impl<H, V>(has_hypervisor: H, vendor: V) -> String
where
    H: FnOnce() -> bool,
    V: FnOnce() -> String,
{
    let has_hypervisor = has_hypervisor();
    let vendor = if has_hypervisor {
        vendor()
    } else {
        String::new()
    };
    virtualization_from_cpuid_parts(has_hypervisor, &vendor)
}

fn cpuid_vendor_string(ebx: u32, ecx: u32, edx: u32) -> String {
    let mut bytes = Vec::with_capacity(12);
    bytes.extend_from_slice(&ebx.to_le_bytes());
    bytes.extend_from_slice(&ecx.to_le_bytes());
    bytes.extend_from_slice(&edx.to_le_bytes());
    String::from_utf8_lossy(&bytes)
        .trim_matches('\0')
        .trim()
        .to_string()
}

fn collect_sysfs_drm_gpu_name() -> Option<String> {
    let entries = fs::read_dir("/sys/class/drm").ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("card") {
            continue;
        }

        let path = entry.path();
        let driver_name = fs::read_link(path.join("device").join("driver"))
            .ok()
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            });
        let Some(driver_name) = driver_name else {
            continue;
        };

        let compatible = fs::read(path.join("device").join("of_node").join("compatible")).ok();
        if let Some(model) = sysfs_drm_gpu_name_from_driver(&driver_name, compatible.as_deref()) {
            return Some(model);
        }
    }

    fs::read_to_string("/sys/firmware/devicetree/base/model")
        .ok()
        .filter(|model| model.contains("Raspberry Pi"))
        .map(|_| "Broadcom VideoCore (Integrated)".to_string())
}

fn is_excluded_drm_driver(driver_name: &str) -> bool {
    const EXCLUDED: &[&str] = &[
        "virtio-pci",
        "virtio_gpu",
        "bochs-drm",
        "qxl",
        "vmwgfx",
        "cirrus",
        "vboxvideo",
        "hyperv_fb",
        "simpledrm",
        "simplefb",
        "cirrus-qemu",
    ];
    EXCLUDED.iter().any(|driver| driver_name == *driver)
}

fn first_token_after_marker(value: &str, marker: &str, digits_only: bool) -> Option<String> {
    let start = value.find(marker)? + marker.len();
    let token = value[start..]
        .chars()
        .take_while(|ch| {
            if digits_only {
                ch.is_ascii_digit()
            } else {
                ch.is_ascii_alphanumeric()
            }
        })
        .collect::<String>();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

fn allwinner_soc_model(value: &str) -> Option<String> {
    let start = value.find("sun")?;
    let suffix = &value[start..];
    let dash = suffix.find('-')? + 1;
    let model = suffix[dash..]
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>();
    if model.is_empty() {
        None
    } else {
        Some(model.to_ascii_uppercase())
    }
}

fn count_tcp_udp_sockets_in_proc_root(proc_root: &Path) -> Result<(i32, i32), String> {
    let tcp =
        count_proc_sockets_in_root(proc_root, &["net/tcp", "net/tcp6"], "TCP", "/proc/net/tcp")?;
    let udp =
        count_proc_sockets_in_root(proc_root, &["net/udp", "net/udp6"], "UDP", "/proc/net/udp")?;

    Ok((tcp, udp))
}

fn count_proc_sockets_in_root(
    proc_root: &Path,
    relative_paths: &[&str],
    label: &str,
    display_path: &str,
) -> Result<i32, String> {
    let mut total = 0;
    let mut read_any = false;

    for relative_path in relative_paths {
        let path = proc_root.join(relative_path);
        if let Ok(contents) = fs::read_to_string(path) {
            read_any = true;
            total += count_socket_entries(&contents);
        }
    }

    if read_any {
        Ok(total)
    } else {
        Err(format!(
            "failed to get {label} connections: {display_path} is not readable"
        ))
    }
}

fn should_include_interface(name: &str) -> bool {
    const EXCLUDED_PREFIXES: &[&str] = &[
        "br", "cni", "docker", "podman", "flannel", "lo", "veth", "virbr", "vmbr",
    ];

    !EXCLUDED_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

fn parse_csv_list(value: &str) -> Vec<String> {
    if value.is_empty() {
        return Vec::new();
    }

    value
        .split(',')
        .map(str::trim)
        .map(ToOwned::to_owned)
        .collect()
}

fn parse_semicolon_list(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn is_physical_disk(mount: &DiskMount) -> bool {
    if mount.mountpoint == "/" {
        return true;
    }

    let mountpoint = mount.mountpoint.to_lowercase();
    const EXCLUDED_MOUNTPOINTS: &[&str] = &[
        "/tmp",
        "/var/tmp",
        "/dev/shm",
        "/run",
        "/run/lock",
        "/run/user",
        "/var/lib/containers",
        "/var/lib/docker",
        "/proc",
        "/dev/pts",
        "/sys",
        "/sys/fs/cgroup",
        "/dev/mqueue",
        "/etc/resolv.conf",
        "/etc/host",
        "/dev/hugepages",
        "/nix/store",
    ];
    if EXCLUDED_MOUNTPOINTS
        .iter()
        .any(|excluded| mountpoint == *excluded || mountpoint.starts_with(excluded))
    {
        return false;
    }

    let fstype = mount.fstype.to_lowercase();
    if fstype == "autofs" && !mount.device.starts_with("/dev/") {
        return false;
    }
    if fstype == "fuseblk" {
        return true;
    }

    const EXCLUDED_FSTYPES: &[&str] = &[
        "tmpfs",
        "devtmpfs",
        "nfs",
        "cifs",
        "smb",
        "vboxsf",
        "9p",
        "fuse",
        "overlay",
        "proc",
        "devpts",
        "sysfs",
        "cgroup",
        "mqueue",
        "hugetlbfs",
    ];
    if EXCLUDED_FSTYPES
        .iter()
        .any(|excluded| fstype == *excluded || fstype.starts_with(excluded))
    {
        return false;
    }

    !mount.device.starts_with("/dev/loop")
}

fn disk_device_id(mount: &DiskMount) -> String {
    if mount.fstype.eq_ignore_ascii_case("zfs") {
        return mount
            .device
            .split_once('/')
            .map(|(pool, _)| pool.to_string())
            .unwrap_or_else(|| mount.device.clone());
    }
    mount.device.clone()
}

#[cfg(target_os = "linux")]
fn disk_usage_for_mountpoint(path: &str) -> Option<DiskValues> {
    let path = CString::new(path).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return None;
    }

    let stat = unsafe { stat.assume_init() };
    let block_size = if stat.f_frsize > 0 {
        stat.f_frsize
    } else {
        stat.f_bsize
    } as u128;
    let total = stat.f_blocks as u128 * block_size;
    let used = stat.f_blocks.saturating_sub(stat.f_bfree) as u128 * block_size;

    Some(DiskValues {
        total: u128_to_i64_saturating(total),
        used: u128_to_i64_saturating(used),
    })
}

#[cfg(not(target_os = "linux"))]
fn disk_usage_for_mountpoint(_path: &str) -> Option<DiskValues> {
    None
}

#[cfg(target_os = "linux")]
fn u128_to_i64_saturating(value: u128) -> i64 {
    value.min(i64::MAX as u128) as i64
}

fn is_display_pci_line(lower_line: &str) -> bool {
    lower_line.contains("vga") || lower_line.contains("3d") || lower_line.contains("display")
}

fn extract_lspci_device_name(line: &str) -> Option<String> {
    let (_, name) = line.rsplit_once(':')?;
    let mut name = name.trim();
    if let Some((before_suffix, _)) = name.rsplit_once('(') {
        name = before_suffix.trim();
    }
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn is_excluded_gpu_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("1111")
        || lower.contains("cirrus logic")
        || lower.contains("virtio")
        || lower.contains("vmware")
        || lower.contains("qxl")
        || lower.contains("hyper-v")
}

fn find_go_ipv4_regex_match(contents: &str) -> Option<&str> {
    let bytes = contents.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        if !bytes[start].is_ascii_digit() {
            start += 1;
            continue;
        }

        let mut cursor = start;
        if consume_ipv4_digits(bytes, &mut cursor).is_none() {
            start += 1;
            continue;
        }

        let mut matched = true;
        for _ in 0..3 {
            if bytes.get(cursor) != Some(&b'.') {
                matched = false;
                break;
            }
            cursor += 1;
            if consume_ipv4_digits(bytes, &mut cursor).is_none() {
                matched = false;
                break;
            }
        }

        if matched {
            return Some(&contents[start..cursor]);
        }
        start += 1;
    }

    None
}

fn consume_ipv4_digits(bytes: &[u8], cursor: &mut usize) -> Option<()> {
    let start = *cursor;
    while *cursor < bytes.len() && bytes[*cursor].is_ascii_digit() && *cursor - start < 3 {
        *cursor += 1;
    }
    (*cursor > start).then_some(())
}

fn find_go_ipv6_regex_match(contents: &str) -> Option<&str> {
    let bytes = contents.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        if !bytes[start].is_ascii_hexdigit() {
            start += 1;
            continue;
        }

        if let Some(end) =
            match_go_ipv6_full(bytes, start).or_else(|| match_go_ipv6_compressed(bytes, start))
        {
            return Some(&contents[start..end]);
        }
        start += 1;
    }

    None
}

fn match_go_ipv6_full(bytes: &[u8], start: usize) -> Option<usize> {
    let mut cursor = start;
    for _ in 0..7 {
        consume_hex_group(bytes, &mut cursor)?;
        if bytes.get(cursor) != Some(&b':') {
            return None;
        }
        cursor += 1;
    }
    consume_hex_group(bytes, &mut cursor)?;
    Some(cursor)
}

fn match_go_ipv6_compressed(bytes: &[u8], start: usize) -> Option<usize> {
    for prefix_groups in (1..=6).rev() {
        let mut cursor = start;
        let mut matched_prefix = true;
        for _ in 0..prefix_groups {
            if consume_hex_group(bytes, &mut cursor).is_none() || bytes.get(cursor) != Some(&b':') {
                matched_prefix = false;
                break;
            }
            cursor += 1;
        }
        if !matched_prefix || bytes.get(cursor) != Some(&b':') {
            continue;
        }
        cursor += 1;

        for _ in 0..4 {
            let before = cursor;
            if consume_hex_group(bytes, &mut cursor).is_some() && bytes.get(cursor) == Some(&b':') {
                cursor += 1;
            } else {
                cursor = before;
                break;
            }
        }
        let _ = consume_hex_group(bytes, &mut cursor);
        return Some(cursor);
    }

    None
}

fn consume_hex_group(bytes: &[u8], cursor: &mut usize) -> Option<()> {
    let start = *cursor;
    while *cursor < bytes.len() && bytes[*cursor].is_ascii_hexdigit() && *cursor - start < 4 {
        *cursor += 1;
    }
    (*cursor > start).then_some(())
}

fn actual_reset_date(year: i32, month: u32, reset_day: u32) -> (i32, u32, u32) {
    let last_day = days_in_month(year, month);
    if reset_day <= last_day {
        return (year, month, reset_day);
    }

    if month == 12 {
        (year + 1, 1, 1)
    } else {
        (year, month + 1, 1)
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 31,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn xml_tag_text(contents: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let (_, after_open) = contents.split_once(&open)?;
    let (value, _) = after_open.split_once(&close)?;
    Some(value.trim().to_string())
}

fn xml_child_tag_text(contents: &str, parent: &str, tag: &str) -> Option<String> {
    let parent_body = xml_tag_text(contents, parent)?;
    xml_tag_text(&parent_body, tag)
}

fn parse_mib_value(value: &str) -> i64 {
    parse_unsigned_i64_value(value).saturating_mul(1024 * 1024)
}

fn parse_unsigned_i64_value(value: &str) -> i64 {
    value
        .trim()
        .trim_end_matches("MiB")
        .trim_end_matches('C')
        .trim()
        .parse::<u64>()
        .ok()
        .and_then(|value| i64::try_from(value).ok())
        .unwrap_or(0)
}

fn parse_percent_value(value: &str) -> f64 {
    value
        .trim()
        .trim_end_matches('%')
        .trim()
        .parse::<f64>()
        .unwrap_or(0.0)
}

fn parse_temperature_value(value: &str) -> i64 {
    parse_unsigned_i64_value(value)
}

fn json_string(card: &serde_json::Map<String, serde_json::Value>, key: &str) -> String {
    card.get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}
