use kelicloud_agent_rs::cn_connectivity::{
    CnConnectivityControlMessageHandler, CnConnectivityReportGenerator, CnConnectivityState,
};
use kelicloud_agent_rs::config::AgentConfig;
use kelicloud_agent_rs::ping::LinuxPingExecutor;
use kelicloud_agent_rs::runtime::{
    run_once_with_ping, run_report_cycles_with_ping_delay, startup_summary, ThreadLoopDelay,
};
use kelicloud_agent_rs::system::{SystemReportGenerator, SystemSnapshotCollector};
use kelicloud_agent_rs::transport::{ReqwestHttpTransport, TungsteniteWebSocketTransport};

fn main() {
    if !kelicloud_agent_rs::linux_proc::linux_supported() {
        eprintln!("kelicloud-agent-rs supports Linux only");
        std::process::exit(2);
    }

    let config = match AgentConfig::from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("configuration error: {error}");
            eprintln!(
                "usage: kelicloud-agent-rs --endpoint https://panel.example.com --token TOKEN"
            );
            std::process::exit(2);
        }
    };

    println!("{}", startup_summary(&config));

    let mut collector = SystemSnapshotCollector::from_config(&config);
    let basic_info = collector.collect().to_basic_info(env!("CARGO_PKG_VERSION"));
    let cn_connectivity_state = CnConnectivityState::default();
    cn_connectivity_state.start_probe_loop();
    let report_generator = CnConnectivityReportGenerator::new(
        SystemReportGenerator::new(collector),
        cn_connectivity_state.clone(),
    );
    let mut http = match ReqwestHttpTransport::new(config.insecure) {
        Ok(http) => http,
        Err(error) => {
            eprintln!("transport error: {error}");
            std::process::exit(2);
        }
    };
    let mut websocket = TungsteniteWebSocketTransport;
    let mut handler = CnConnectivityControlMessageHandler::new(cn_connectivity_state);
    let ping_executor = LinuxPingExecutor::default();

    let result = if config.once {
        run_once_with_ping(
            &config,
            &basic_info,
            &report_generator,
            &ping_executor,
            &mut http,
            &mut websocket,
            &mut handler,
        )
    } else {
        let mut delay = ThreadLoopDelay;
        run_report_cycles_with_ping_delay(
            &config,
            &basic_info,
            &report_generator,
            &ping_executor,
            &mut http,
            &mut websocket,
            &mut handler,
            &mut delay,
            usize::MAX,
        )
    };

    if let Err(error) = result {
        eprintln!("runtime error: {error}");
        std::process::exit(1);
    }

    println!("agent loop: completed");
}
