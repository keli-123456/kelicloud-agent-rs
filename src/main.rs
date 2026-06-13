use kelicloud_agent_rs::cn_connectivity::{
    CnConnectivityControlMessageHandler, CnConnectivityReportGenerator, CnConnectivityState,
};
use kelicloud_agent_rs::config::AgentConfig;
use kelicloud_agent_rs::ping::LinuxPingExecutor;
use kelicloud_agent_rs::runtime::{
    run_once_with_ping, run_report_cycles_with_ping_delay, startup_summary,
    ChainControlMessageHandler, ThreadLoopDelay,
};
use kelicloud_agent_rs::system::{SystemReportGenerator, SystemSnapshotCollector};
use kelicloud_agent_rs::task::{
    HttpTaskResultUploader, LinuxTaskExecutor, TaskControlMessageHandler,
};
use kelicloud_agent_rs::terminal::{TerminalControlMessageHandler, TungsteniteTerminalConnector};
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

    let basic_info_provider = || {
        let mut collector = SystemSnapshotCollector::from_config(&config);
        collector.collect().to_basic_info(env!("CARGO_PKG_VERSION"))
    };
    let collector = SystemSnapshotCollector::from_config(&config);
    let cn_connectivity_state = CnConnectivityState::default();
    cn_connectivity_state.start_probe_loop();
    let report_generator = CnConnectivityReportGenerator::new(
        SystemReportGenerator::new(collector),
        cn_connectivity_state.clone(),
    );
    let mut http = match ReqwestHttpTransport::from_config(&config) {
        Ok(http) => http,
        Err(error) => {
            eprintln!("transport error: {error}");
            std::process::exit(2);
        }
    };
    let mut websocket = TungsteniteWebSocketTransport::from_config(&config);
    let task_uploader = match HttpTaskResultUploader::from_config(&config) {
        Ok(uploader) => uploader,
        Err(error) => {
            eprintln!("task uploader error: {error}");
            std::process::exit(2);
        }
    };
    let cn_handler = CnConnectivityControlMessageHandler::new(cn_connectivity_state);
    let task_handler =
        TaskControlMessageHandler::new(LinuxTaskExecutor, task_uploader, config.disable_web_ssh);
    let terminal_handler = TerminalControlMessageHandler::new(
        TungsteniteTerminalConnector::from_config(&config),
        config.disable_web_ssh,
    );
    let control_handler = ChainControlMessageHandler::new(cn_handler, task_handler);
    let mut handler = ChainControlMessageHandler::new(control_handler, terminal_handler);
    let ping_executor = LinuxPingExecutor::default();

    let result = if config.once {
        run_once_with_ping(
            &config,
            &basic_info_provider,
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
            &basic_info_provider,
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
