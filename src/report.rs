use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub cpu: CpuReport,
    pub ram: MemoryReport,
    pub swap: MemoryReport,
    pub load: LoadReport,
    pub disk: DiskReport,
    pub network: NetworkReport,
    pub connections: ConnectionsReport,
    pub uptime: i64,
    pub process: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<GpuReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cn_connectivity: Option<serde_json::Value>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CpuReport {
    pub usage: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryReport {
    pub total: i64,
    pub used: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LoadReport {
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskReport {
    pub total: i64,
    pub used: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkReport {
    pub up: i64,
    pub down: i64,
    #[serde(rename = "totalUp")]
    pub total_up: i64,
    #[serde(rename = "totalDown")]
    pub total_down: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionsReport {
    pub tcp: i32,
    pub udp: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_usage: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detailed_info: Option<Vec<GpuDetailedInfo>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuDetailedInfo {
    pub name: String,
    pub memory_total: i64,
    pub memory_used: i64,
    pub utilization: f64,
    pub temperature: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BasicInfo {
    pub cpu_name: String,
    pub cpu_cores: i32,
    pub arch: String,
    pub os: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub kernel_version: String,
    pub ipv4: String,
    pub ipv6: String,
    pub mem_total: i64,
    pub swap_total: i64,
    pub disk_total: i64,
    pub gpu_name: String,
    pub virtualization: String,
    pub version: String,
}

impl BasicInfo {
    pub fn minimal(version: &str) -> Self {
        let cpu_cores = std::thread::available_parallelism()
            .map(|cores| cores.get() as i32)
            .unwrap_or(1);

        Self {
            cpu_name: String::new(),
            cpu_cores,
            arch: go_runtime_arch_name(std::env::consts::ARCH).to_string(),
            os: std::env::consts::OS.to_string(),
            kernel_version: String::new(),
            ipv4: String::new(),
            ipv6: String::new(),
            mem_total: 0,
            swap_total: 0,
            disk_total: 0,
            gpu_name: String::new(),
            virtualization: String::new(),
            version: version.to_string(),
        }
    }

    pub fn without_kernel_version(&self) -> Self {
        let mut basic_info = self.clone();
        basic_info.kernel_version.clear();
        basic_info
    }
}

pub fn go_runtime_arch_name(rust_arch: &str) -> &str {
    match rust_arch {
        "x86_64" => "amd64",
        "x86" | "i386" | "i586" | "i686" => "386",
        "aarch64" => "arm64",
        "powerpc64" => "ppc64",
        "powerpc64le" => "ppc64le",
        other => other,
    }
}

pub trait ReportGenerator {
    fn generate(&self) -> Report;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct StaticReportGenerator;

impl ReportGenerator for StaticReportGenerator {
    fn generate(&self) -> Report {
        Report {
            cpu: CpuReport { usage: 0.001 },
            ram: MemoryReport { total: 0, used: 0 },
            swap: MemoryReport { total: 0, used: 0 },
            load: LoadReport {
                load1: 0.0,
                load5: 0.0,
                load15: 0.0,
            },
            disk: DiskReport { total: 0, used: 0 },
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
