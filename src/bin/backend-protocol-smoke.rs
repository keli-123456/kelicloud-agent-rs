use kelicloud_agent_rs::backend_protocol_smoke::run_backend_protocol_smoke;
use kelicloud_agent_rs::config::AgentConfig;

fn main() {
    let config = match AgentConfig::from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("configuration error: {error}");
            eprintln!(
                "usage: backend-protocol-smoke --endpoint https://panel.example.com (--token TOKEN | --auto-discovery KEY)"
            );
            std::process::exit(2);
        }
    };

    if let Err(error) = run_backend_protocol_smoke(config) {
        eprintln!("backend protocol smoke failed: {error}");
        std::process::exit(1);
    }

    println!("agent loop: completed");
}
