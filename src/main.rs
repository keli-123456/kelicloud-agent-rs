use kelicloud_agent_rs::cn_connectivity::{
    CnConnectivityControlMessageHandler, CnConnectivityReportGenerator, CnConnectivityState,
};
use kelicloud_agent_rs::config::AgentConfig;
use kelicloud_agent_rs::ping::LinuxPingExecutor;
use kelicloud_agent_rs::protocol::{build_tunnel_control_ws_url, build_tunnel_data_ws_url};
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
use kelicloud_agent_rs::transport::{
    access_headers, ReqwestHttpTransport, TungsteniteWebSocketTransport,
};
use kelicloud_agent_rs::tunnel_control::{
    run_tunnel_control_once, tunnel_control_startup_line, TungsteniteTunnelControlTransport,
};
use kelicloud_agent_rs::tunnel_data::{
    run_tunnel_data_once, tunnel_data_startup_line, TungsteniteTunnelDataTransport,
    TunnelDataReadyState,
};

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
    let tunnel_control_url =
        build_tunnel_control_ws_url(&config.endpoint, &shared_token.get()).ok();
    if let Some(url) = tunnel_control_url.as_deref() {
        println!(
            "{}",
            tunnel_control_startup_line(url, config.tunnel_control_enabled)
        );
    } else if config.tunnel_control_enabled {
        println!("tunnel control: enabled url=invalid");
    } else {
        println!("{}", tunnel_control_startup_line("", false));
    }
    let tunnel_data_url = build_tunnel_data_ws_url(&config.endpoint, &shared_token.get()).ok();
    if let Some(url) = tunnel_data_url.as_deref() {
        println!(
            "{}",
            tunnel_data_startup_line(url, config.tunnel_data_enabled)
        );
    } else if config.tunnel_data_enabled {
        println!("tunnel data: enabled url=invalid");
    } else {
        println!("{}", tunnel_data_startup_line("", false));
    }

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

    if config.tunnel_control_enabled {
        let tunnel_headers = access_headers(&config);
        let tunnel_endpoint = config.endpoint.clone();
        let tunnel_custom_dns = config.custom_dns.clone();
        let tunnel_agent_version = env!("CARGO_PKG_VERSION").to_string();
        if config.once {
            if let Ok(url) = build_tunnel_control_ws_url(&tunnel_endpoint, &shared_token.get()) {
                let mut tunnel_transport =
                    TungsteniteTunnelControlTransport::new_with_custom_dns(&tunnel_custom_dns);
                if let Err(error) = run_tunnel_control_once(
                    &url,
                    &tunnel_headers,
                    &tunnel_agent_version,
                    &mut tunnel_transport,
                ) {
                    eprintln!("tunnel control warning: {error}");
                }
            }
        } else {
            let tunnel_shared_token = shared_token.clone();
            std::thread::spawn(move || {
                let mut retry_delay = std::time::Duration::from_secs(5);
                loop {
                    match build_tunnel_control_ws_url(&tunnel_endpoint, &tunnel_shared_token.get())
                    {
                        Ok(url) => {
                            let mut tunnel_transport =
                                TungsteniteTunnelControlTransport::new_with_custom_dns(
                                    &tunnel_custom_dns,
                                );
                            match run_tunnel_control_once(
                                &url,
                                &tunnel_headers,
                                &tunnel_agent_version,
                                &mut tunnel_transport,
                            ) {
                                Ok(()) => retry_delay = std::time::Duration::from_secs(15),
                                Err(error) => eprintln!("tunnel control warning: {error}"),
                            }
                        }
                        Err(error) => eprintln!("tunnel control warning: {error}"),
                    }
                    std::thread::sleep(retry_delay);
                    retry_delay =
                        (retry_delay + retry_delay).min(std::time::Duration::from_secs(60));
                }
            });
        }
    }

    if config.tunnel_data_enabled {
        let tunnel_data_headers = access_headers(&config);
        let tunnel_data_endpoint = config.endpoint.clone();
        let tunnel_data_custom_dns = config.custom_dns.clone();
        let tunnel_data_agent_version = env!("CARGO_PKG_VERSION").to_string();
        let tunnel_data_shared_token = shared_token.clone();
        std::thread::spawn(move || {
            let ready = TunnelDataReadyState::empty("");
            loop {
                match build_tunnel_data_ws_url(
                    &tunnel_data_endpoint,
                    &tunnel_data_shared_token.get(),
                ) {
                    Ok(url) => {
                        let mut transport = TungsteniteTunnelDataTransport::new_with_custom_dns(
                            &tunnel_data_custom_dns,
                        );
                        if let Err(error) = run_tunnel_data_once(
                            &url,
                            &tunnel_data_headers,
                            "",
                            &tunnel_data_agent_version,
                            &ready,
                            &mut transport,
                        ) {
                            eprintln!("tunnel data warning: {error}");
                        }
                    }
                    Err(error) => eprintln!("tunnel data warning: {error}"),
                }
                std::thread::sleep(std::time::Duration::from_secs(30));
            }
        });
    }

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
