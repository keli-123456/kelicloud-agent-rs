use crate::config::AgentConfig;
use crate::linux_proc::{GpuMetric, ProcMetricErrors};
use crate::net_static::{InterfaceCounter, NetStaticSampler, NetStaticSamplerConfig};
use crate::report::{
    go_runtime_arch_name, BasicInfo, ConnectionsReport, CpuReport, DiskReport, GpuDetailedInfo,
    GpuReport, LoadReport, MemoryReport, NetworkReport, Report, ReportGenerator,
};
use std::cell::RefCell;
use std::fmt::Write;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct SystemSnapshot {
    pub cpu_name: String,
    pub cpu_cores: i32,
    pub arch: String,
    pub os: String,
    pub kernel_version: String,
    pub ipv4: String,
    pub ipv6: String,
    pub mem_total: i64,
    pub mem_used: i64,
    pub swap_total: i64,
    pub swap_used: i64,
    pub disk_total: i64,
    pub disk_used: i64,
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
    pub network_up: i64,
    pub network_down: i64,
    pub network_total_up: i64,
    pub network_total_down: i64,
    pub tcp_connections: i32,
    pub udp_connections: i32,
    pub uptime: i64,
    pub process_count: i32,
    pub cpu_usage: f64,
    pub virtualization: String,
    pub gpu_name: String,
    pub message: String,
    pub gpu_report: Option<GpuReport>,
    pub cn_connectivity: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemMetricsOptions {
    pub include_nics: String,
    pub exclude_nics: String,
    pub include_mountpoints: String,
    pub custom_ipv4: String,
    pub custom_ipv6: String,
    pub get_ip_addr_from_nic: bool,
    pub memory_include_cache: bool,
    pub memory_report_raw_used: bool,
    pub enable_gpu: bool,
    pub month_rotate: u32,
    pub host_proc: String,
    pub public_ip_probe: bool,
    pub net_static_path: String,
    pub network_speed_sample_millis: u64,
}

impl Default for SystemMetricsOptions {
    fn default() -> Self {
        Self {
            include_nics: String::new(),
            exclude_nics: String::new(),
            include_mountpoints: String::new(),
            custom_ipv4: String::new(),
            custom_ipv6: String::new(),
            get_ip_addr_from_nic: false,
            memory_include_cache: false,
            memory_report_raw_used: false,
            enable_gpu: false,
            month_rotate: 0,
            host_proc: String::new(),
            public_ip_probe: false,
            net_static_path: "net_static.json".to_string(),
            network_speed_sample_millis: 1_000,
        }
    }
}

impl From<&AgentConfig> for SystemMetricsOptions {
    fn from(config: &AgentConfig) -> Self {
        Self {
            include_nics: config.include_nics.clone(),
            exclude_nics: config.exclude_nics.clone(),
            include_mountpoints: config.include_mountpoints.clone(),
            custom_ipv4: config.custom_ipv4.clone(),
            custom_ipv6: config.custom_ipv6.clone(),
            get_ip_addr_from_nic: config.get_ip_addr_from_nic,
            memory_include_cache: config.memory_include_cache,
            memory_report_raw_used: config.memory_report_raw_used,
            enable_gpu: config.enable_gpu,
            month_rotate: config.month_rotate,
            host_proc: config.host_proc.clone(),
            public_ip_probe: true,
            net_static_path: "net_static.json".to_string(),
            network_speed_sample_millis: 1_000,
        }
    }
}

impl SystemMetricsOptions {
    fn net_static_path(&self) -> &str {
        if self.net_static_path.is_empty() {
            "net_static.json"
        } else {
            &self.net_static_path
        }
    }
}

pub fn gpu_report_from_metrics(gpu_name: &str, metrics: Vec<GpuMetric>) -> Option<GpuReport> {
    if !metrics.is_empty() {
        let count = metrics.len();
        let average_usage =
            metrics.iter().map(|metric| metric.utilization).sum::<f64>() / count as f64;
        return Some(GpuReport {
            models: None,
            count: Some(count),
            average_usage: Some(average_usage),
            detailed_info: Some(
                metrics
                    .into_iter()
                    .map(|metric| GpuDetailedInfo {
                        name: metric.name,
                        memory_total: metric.memory_total,
                        memory_used: metric.memory_used,
                        utilization: metric.utilization,
                        temperature: metric.temperature,
                    })
                    .collect(),
            ),
        });
    }

    if gpu_name != "None" && !gpu_name.is_empty() {
        return Some(GpuReport {
            models: Some(vec![gpu_name.to_string()]),
            count: None,
            average_usage: None,
            detailed_info: None,
        });
    }

    None
}

pub fn append_report_error(message: &mut String, label: &str, error: impl std::fmt::Display) {
    let _ = writeln!(message, "failed to get {label}: {error}");
}

pub fn gpu_report_from_detailed_result(
    gpu_name: &str,
    result: Result<Vec<GpuMetric>, String>,
) -> (Option<GpuReport>, String) {
    match result {
        Ok(metrics) => (gpu_report_from_metrics(gpu_name, metrics), String::new()),
        Err(error) => {
            let mut message = String::new();
            append_report_error(&mut message, "detailed GPU info", error);
            (gpu_report_from_metrics(gpu_name, Vec::new()), message)
        }
    }
}

pub fn proc_metric_errors_to_message(errors: &ProcMetricErrors) -> String {
    let mut message = String::new();

    if let Some(error) = errors.network_speed.as_ref() {
        append_report_error(&mut message, "network speed", error);
    }
    if let Some(error) = errors.connections.as_ref() {
        append_report_error(&mut message, "connections", error);
    }
    if let Some(error) = errors.uptime.as_ref() {
        append_report_error(&mut message, "uptime", error);
    }

    message
}

pub fn go_compatible_cpu_usage(usage: f64) -> f64 {
    if usage <= 0.001 {
        0.001
    } else {
        usage
    }
}

pub fn select_basic_info_ip_addresses(
    get_ip_addr_from_nic: bool,
    nic_addresses: Option<crate::linux_proc::IpAddresses>,
    mut fallback_addresses: crate::linux_proc::IpAddresses,
    custom_ipv4: &str,
    custom_ipv6: &str,
) -> crate::linux_proc::IpAddresses {
    if get_ip_addr_from_nic {
        if let Some(addresses) = nic_addresses {
            if !addresses.ipv4.is_empty() || !addresses.ipv6.is_empty() {
                return addresses;
            }
        }
    }

    if !custom_ipv4.is_empty() {
        fallback_addresses.ipv4 = custom_ipv4.to_string();
    }
    if !custom_ipv6.is_empty() {
        fallback_addresses.ipv6 = custom_ipv6.to_string();
    }
    fallback_addresses
}

impl SystemSnapshot {
    pub fn to_report(&self) -> Report {
        Report {
            cpu: CpuReport {
                usage: go_compatible_cpu_usage(self.cpu_usage),
            },
            ram: MemoryReport {
                total: self.mem_total,
                used: self.mem_used,
            },
            swap: MemoryReport {
                total: self.swap_total,
                used: self.swap_used,
            },
            load: LoadReport {
                load1: self.load1,
                load5: self.load5,
                load15: self.load15,
            },
            disk: DiskReport {
                total: self.disk_total,
                used: self.disk_used,
            },
            network: NetworkReport {
                up: self.network_up,
                down: self.network_down,
                total_up: self.network_total_up,
                total_down: self.network_total_down,
            },
            connections: ConnectionsReport {
                tcp: self.tcp_connections,
                udp: self.udp_connections,
            },
            uptime: self.uptime,
            process: self.process_count,
            gpu: self.gpu_report.clone(),
            cn_connectivity: self.cn_connectivity.clone(),
            message: self.message.clone(),
        }
    }

    pub fn to_basic_info(&self, version: &str) -> BasicInfo {
        BasicInfo {
            cpu_name: self.cpu_name.clone(),
            cpu_cores: self.cpu_cores,
            arch: self.arch.clone(),
            os: self.os.clone(),
            kernel_version: self.kernel_version.clone(),
            ipv4: self.ipv4.clone(),
            ipv6: self.ipv6.clone(),
            mem_total: self.mem_total,
            swap_total: self.swap_total,
            disk_total: self.disk_total,
            gpu_name: self.gpu_name.clone(),
            virtualization: self.virtualization.clone(),
            version: version.to_string(),
        }
    }
}

#[derive(Debug)]
pub struct SystemSnapshotCollector {
    system: sysinfo::System,
    metrics: SystemMetricsOptions,
    net_static_sampler: Option<NetStaticSampler>,
}

impl SystemSnapshotCollector {
    pub fn new() -> Self {
        Self::with_metrics(SystemMetricsOptions::default())
    }

    pub fn from_config(config: &AgentConfig) -> Self {
        Self::with_metrics(SystemMetricsOptions::from(config))
    }

    pub fn with_metrics(metrics: SystemMetricsOptions) -> Self {
        let mut system = sysinfo::System::new_all();
        system.refresh_all();
        let net_static_sampler = Self::net_static_sampler_for_metrics(&metrics);
        Self {
            system,
            metrics,
            net_static_sampler,
        }
    }

    pub fn collect(&mut self) -> SystemSnapshot {
        self.system.refresh_all();
        let network_filter = crate::linux_proc::NetworkFilter::from_csv(
            &self.metrics.include_nics,
            &self.metrics.exclude_nics,
        );
        let proc_sample = crate::linux_proc::collect_proc_metrics_sample_with_filter_and_host_proc(
            &network_filter,
            &self.metrics.host_proc,
        );
        let proc_metrics = proc_sample.as_ref().map(|sample| sample.metrics);
        let mut message = proc_sample
            .as_ref()
            .map(|sample| proc_metric_errors_to_message(&sample.errors))
            .unwrap_or_default();
        let linux_memory =
            crate::linux_proc::collect_memory_values_with_mode(self.metrics.memory_include_cache);

        let cpus = self.system.cpus();
        let cpu_name = crate::linux_proc::collect_cpu_name(cpus.first().map(|cpu| cpu.brand()));
        let cpu_usage = if cpus.is_empty() {
            0.001
        } else {
            let total = cpus
                .iter()
                .map(|cpu| f64::from(cpu.cpu_usage()))
                .sum::<f64>();
            total / cpus.len() as f64
        };

        let disks = sysinfo::Disks::new_with_refreshed_list();
        let disk_mounts = disks
            .iter()
            .map(|disk| {
                let total = disk.total_space() as i64;
                let available = disk.available_space() as i64;
                crate::linux_proc::DiskMount {
                    device: disk.name().to_string_lossy().into_owned(),
                    mountpoint: disk.mount_point().to_string_lossy().into_owned(),
                    fstype: disk.file_system().to_string_lossy().into_owned(),
                    total,
                    used: total.saturating_sub(available),
                }
            })
            .collect::<Vec<_>>();
        let disk_values = crate::linux_proc::go_compatible_disk_with_mountpoints(
            &disk_mounts,
            &self.metrics.include_mountpoints,
        );

        let first_network = proc_metrics
            .as_ref()
            .map(|metrics| metrics.network)
            .unwrap_or_default();
        let mut network_speed_sample = crate::linux_proc::NetworkSpeedSample {
            total: first_network,
            speed: crate::linux_proc::NetworkTotals::default(),
        };
        if proc_sample
            .as_ref()
            .is_some_and(|sample| sample.errors.network_speed.is_none())
        {
            if self.metrics.network_speed_sample_millis > 0 {
                std::thread::sleep(Duration::from_millis(
                    self.metrics.network_speed_sample_millis,
                ));
            }
            if let Some(second_network) =
                crate::linux_proc::collect_network_totals_with_filter_and_host_proc(
                    &network_filter,
                    &self.metrics.host_proc,
                )
            {
                match second_network {
                    Ok(second_network) => {
                        network_speed_sample = crate::linux_proc::network_speed_from_samples(
                            first_network,
                            second_network,
                            self.metrics.network_speed_sample_millis as f64 / 1000.0,
                        );
                    }
                    Err(error) => {
                        append_report_error(&mut message, "network speed", error);
                    }
                }
            }
        }
        let now_local = chrono::Local::now();
        let now_unix = now_local.timestamp().max(0) as u64;
        if let Some(sampler) = self.net_static_sampler.as_mut() {
            if let Some(interfaces) =
                crate::linux_proc::collect_network_interfaces_with_filter(&network_filter)
            {
                let counters = interfaces
                    .into_iter()
                    .map(|interface| {
                        InterfaceCounter::new(
                            interface.name,
                            interface.total_up.max(0) as u64,
                            interface.total_down.max(0) as u64,
                        )
                    })
                    .collect::<Vec<_>>();
                sampler.sample(now_unix, &counters);
            }
            let _ = sampler.flush_if_due(now_unix);
        }
        let monthly_network = self.month_rotate_network_totals(now_local, &network_filter);
        let network_totals = monthly_network.unwrap_or(network_speed_sample.total);
        let network_up = network_speed_sample.speed.total_up;
        let network_down = network_speed_sample.speed.total_down;

        let mem_total = linux_memory
            .map(|(ram, _)| ram.total)
            .unwrap_or_else(|| self.system.total_memory() as i64);
        let mem_used = linux_memory
            .map(|(ram, _)| ram.used)
            .unwrap_or_else(|| self.system.used_memory() as i64);
        let swap_total = linux_memory
            .map(|(_, swap)| swap.total)
            .unwrap_or_else(|| self.system.total_swap() as i64);
        let swap_used = linux_memory
            .map(|(_, swap)| swap.used)
            .unwrap_or_else(|| self.system.used_swap() as i64);
        let nic_addresses = if self.metrics.get_ip_addr_from_nic {
            crate::linux_proc::collect_ip_addresses_with_filter(&network_filter)
        } else {
            None
        };
        let nic_has_addresses = nic_addresses
            .as_ref()
            .is_some_and(|addresses| !addresses.ipv4.is_empty() || !addresses.ipv6.is_empty());
        let fallback_addresses = if self.metrics.get_ip_addr_from_nic && nic_has_addresses {
            crate::linux_proc::IpAddresses::default()
        } else if self.metrics.public_ip_probe {
            crate::linux_proc::collect_public_ip_addresses()
        } else {
            crate::linux_proc::IpAddresses::default()
        };
        let ip_addresses = select_basic_info_ip_addresses(
            self.metrics.get_ip_addr_from_nic,
            nic_addresses,
            fallback_addresses,
            &self.metrics.custom_ipv4,
            &self.metrics.custom_ipv6,
        );
        let virtualization = crate::linux_proc::detect_virtualization();
        let gpu_name = crate::linux_proc::collect_gpu_name();
        let (gpu_report, gpu_message) = if self.metrics.enable_gpu {
            gpu_report_from_detailed_result(
                &gpu_name,
                crate::linux_proc::collect_detailed_gpu_metrics_result(),
            )
        } else {
            (None, String::new())
        };
        message.push_str(&gpu_message);

        SystemSnapshot {
            cpu_name,
            cpu_cores: cpus.len().max(1) as i32,
            arch: go_runtime_arch_name(std::env::consts::ARCH).to_string(),
            os: crate::linux_proc::collect_os_name(),
            kernel_version: crate::linux_proc::collect_kernel_version(),
            ipv4: ip_addresses.ipv4,
            ipv6: ip_addresses.ipv6,
            mem_total,
            mem_used,
            swap_total,
            swap_used,
            disk_total: disk_values.total,
            disk_used: disk_values.used,
            load1: proc_metrics
                .as_ref()
                .map(|metrics| metrics.load1)
                .unwrap_or(0.0),
            load5: proc_metrics
                .as_ref()
                .map(|metrics| metrics.load5)
                .unwrap_or(0.0),
            load15: proc_metrics
                .as_ref()
                .map(|metrics| metrics.load15)
                .unwrap_or(0.0),
            network_up,
            network_down,
            network_total_up: network_totals.total_up,
            network_total_down: network_totals.total_down,
            tcp_connections: proc_metrics
                .as_ref()
                .map(|metrics| metrics.tcp_connections)
                .unwrap_or(0),
            udp_connections: proc_metrics
                .as_ref()
                .map(|metrics| metrics.udp_connections)
                .unwrap_or(0),
            uptime: proc_metrics
                .as_ref()
                .map(|metrics| metrics.uptime)
                .unwrap_or_else(|| sysinfo::System::uptime() as i64),
            process_count: proc_metrics
                .as_ref()
                .map(|metrics| metrics.process_count)
                .filter(|count| *count > 0)
                .unwrap_or_else(|| self.system.processes().len() as i32),
            cpu_usage,
            virtualization,
            gpu_name,
            message,
            gpu_report,
            cn_connectivity: None,
        }
    }

    fn net_static_sampler_for_metrics(metrics: &SystemMetricsOptions) -> Option<NetStaticSampler> {
        if metrics.month_rotate == 0 {
            return None;
        }

        Some(NetStaticSampler::with_config(NetStaticSamplerConfig {
            path: metrics.net_static_path().into(),
            nics: parse_nics(&metrics.include_nics),
            ..NetStaticSamplerConfig::default()
        }))
    }

    fn month_rotate_network_totals(
        &self,
        now: chrono::DateTime<chrono::Local>,
        network_filter: &crate::linux_proc::NetworkFilter,
    ) -> Option<crate::linux_proc::NetworkTotals> {
        if self.metrics.month_rotate == 0 {
            return None;
        }

        let start = crate::linux_proc::reset_timestamp_for_day(self.metrics.month_rotate, now)?;
        let end = now.timestamp().max(0) as u64;
        if let Some(sampler) = self.net_static_sampler.as_ref() {
            return Some(sampler.total_between(start, end, network_filter));
        }

        crate::linux_proc::collect_net_static_total_between(
            self.metrics.net_static_path(),
            start,
            end,
            network_filter,
        )
    }
}

fn parse_nics(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect()
}

impl Default for SystemSnapshotCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct SystemReportGenerator {
    collector: RefCell<SystemSnapshotCollector>,
}

impl SystemReportGenerator {
    pub fn new(collector: SystemSnapshotCollector) -> Self {
        Self {
            collector: RefCell::new(collector),
        }
    }
}

impl Default for SystemReportGenerator {
    fn default() -> Self {
        Self::new(SystemSnapshotCollector::new())
    }
}

impl ReportGenerator for SystemReportGenerator {
    fn generate(&self) -> Report {
        self.collector.borrow_mut().collect().to_report()
    }
}
