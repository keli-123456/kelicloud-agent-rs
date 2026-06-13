use std::error::Error;

use crate::config::AgentConfig;
use crate::ping::NoopPingExecutor;
use crate::report::{
    BasicInfo, ConnectionsReport, CpuReport, DiskReport, LoadReport, MemoryReport, NetworkReport,
    Report, ReportGenerator,
};
use crate::runtime::{run_once_with_ping, NoopControlMessageHandler};
use crate::transport::{ReqwestHttpTransport, TungsteniteWebSocketTransport};

const GIB: i64 = 1024 * 1024 * 1024;

#[derive(Debug, Default, Clone, Copy)]
pub struct StaticSmokeReportGenerator;

impl ReportGenerator for StaticSmokeReportGenerator {
    fn generate(&self) -> Report {
        Report {
            cpu: CpuReport { usage: 1.0 },
            ram: MemoryReport {
                total: 2 * GIB,
                used: 512 * 1024 * 1024,
            },
            swap: MemoryReport {
                total: GIB,
                used: 0,
            },
            load: LoadReport {
                load1: 0.01,
                load5: 0.05,
                load15: 0.10,
            },
            disk: DiskReport {
                total: 20 * GIB,
                used: 3 * GIB,
            },
            network: NetworkReport {
                up: 128,
                down: 256,
                total_up: 1024,
                total_down: 2048,
            },
            connections: ConnectionsReport { tcp: 1, udp: 0 },
            uptime: 3600,
            process: 12,
            gpu: None,
            cn_connectivity: None,
            message: "backend-protocol-smoke".to_string(),
        }
    }
}

pub fn sample_basic_info(version: &str) -> BasicInfo {
    let cpu_cores = std::thread::available_parallelism()
        .map(|cores| cores.get() as i32)
        .unwrap_or(1)
        .max(1);

    BasicInfo {
        cpu_name: "Smoke CPU".to_string(),
        cpu_cores,
        arch: "amd64".to_string(),
        os: "linux".to_string(),
        kernel_version: "6.8.0-smoke".to_string(),
        ipv4: "127.0.0.1".to_string(),
        ipv6: String::new(),
        mem_total: 2 * GIB,
        swap_total: GIB,
        disk_total: 20 * GIB,
        gpu_name: String::new(),
        virtualization: "smoke".to_string(),
        version: version.to_string(),
    }
}

pub fn run_backend_protocol_smoke(mut config: AgentConfig) -> Result<(), Box<dyn Error>> {
    crate::auto_discovery::resolve_auto_discovery(&mut config)?;

    let basic_info = sample_basic_info(env!("CARGO_PKG_VERSION"));
    let report_generator = StaticSmokeReportGenerator;
    let ping_executor = NoopPingExecutor;
    let mut handler = NoopControlMessageHandler;
    let mut http = ReqwestHttpTransport::from_config(&config)?;
    let mut websocket = TungsteniteWebSocketTransport::from_config(&config);

    run_once_with_ping(
        &config,
        &basic_info,
        &report_generator,
        &ping_executor,
        &mut http,
        &mut websocket,
        &mut handler,
    )?;

    Ok(())
}
