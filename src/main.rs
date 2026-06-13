use kelicloud_agent_rs::cn_connectivity::{
    CnConnectivityControlMessageHandler, CnConnectivityReportGenerator, CnConnectivityState,
};
use kelicloud_agent_rs::config::AgentConfig;
use kelicloud_agent_rs::ping::LinuxPingExecutor;
use kelicloud_agent_rs::runtime::{
    run_once_with_ping, run_once_with_ping_and_token_recovery, run_report_cycles_with_ping_delay,
    run_report_cycles_with_ping_delay_and_token_recovery, startup_summary,
    ChainControlMessageHandler, ThreadLoopDelay,
};
use kelicloud_agent_rs::system::{SystemReportGenerator, SystemSnapshotCollector};
use kelicloud_agent_rs::task::{
    HttpTaskResultUploader, LinuxTaskExecutor, TaskControlMessageHandler,
};
use kelicloud_agent_rs::terminal::{TerminalControlMessageHandler, TungsteniteTerminalConnector};
use kelicloud_agent_rs::token::{SharedAgentToken, SharedTokenRecovery};
use kelicloud_agent_rs::transport::{ReqwestHttpTransport, TungsteniteWebSocketTransport};

fn main() {
    if !kelicloud_agent_rs::linux_proc::linux_supported() {
        eprintln!("kelicloud-agent-rs supports Linux only");
        std::process::exit(2);
    }

    let mut config = match AgentConfig::from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("configuration error: {error}");
            eprintln!(
                "usage: kelicloud-agent-rs --endpoint https://panel.example.com (--token TOKEN | --auto-discovery KEY)"
            );
            std::process::exit(2);
        }
    };
    if let Err(error) = kelicloud_agent_rs::auto_discovery::resolve_auto_discovery(&mut config) {
        eprintln!("auto-discovery error: {error}");
        std::process::exit(2);
    }

    println!("{}", startup_summary(&config));
    let shared_token = SharedAgentToken::new(config.token.clone());

    let basic_info_config = config.clone();
    let basic_info_provider = move || {
        let mut collector = SystemSnapshotCollector::from_config(&basic_info_config);
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
    let task_uploader =
        match HttpTaskResultUploader::from_config_with_token(&config, shared_token.clone()) {
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
        TungsteniteTerminalConnector::from_config_with_token(&config, shared_token.clone()),
        config.disable_web_ssh,
    );
    let control_handler = ChainControlMessageHandler::new(cn_handler, task_handler);
    let mut handler = ChainControlMessageHandler::new(control_handler, terminal_handler);
    let ping_executor = LinuxPingExecutor::default();

    let auto_discovery_recovery =
        match kelicloud_agent_rs::auto_discovery::token_recovery_from_config(&config) {
            Ok(recovery) => recovery,
            Err(error) => {
                eprintln!("auto-discovery recovery error: {error}");
                std::process::exit(2);
            }
        };

    let result = if let Some(recovery) = auto_discovery_recovery {
        let mut recovery = SharedTokenRecovery::new(recovery, shared_token);
        if config.once {
            run_once_with_ping_and_token_recovery(
                &mut config,
                &basic_info_provider,
                &report_generator,
                &ping_executor,
                &mut http,
                &mut websocket,
                &mut handler,
                &mut recovery,
            )
        } else {
            let mut delay = ThreadLoopDelay;
            run_report_cycles_with_ping_delay_and_token_recovery(
                &mut config,
                &basic_info_provider,
                &report_generator,
                &ping_executor,
                &mut http,
                &mut websocket,
                &mut handler,
                &mut delay,
                &mut recovery,
                usize::MAX,
            )
        }
    } else if config.once {
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
