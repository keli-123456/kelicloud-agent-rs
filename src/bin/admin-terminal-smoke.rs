use std::time::Duration;

use kelicloud_agent_rs::admin_terminal_smoke::{
    run_admin_terminal_smoke, AdminTerminalSmokeRequest,
};

fn main() {
    let config = match parse_args() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            print_usage();
            std::process::exit(2);
        }
    };

    if let Err(error) = run_admin_terminal_smoke(&config) {
        eprintln!("admin terminal smoke failed: {error}");
        std::process::exit(1);
    }

    println!("admin terminal smoke observed expected output");
}

fn parse_args() -> Result<AdminTerminalSmokeRequest, String> {
    let mut endpoint = std::env::var("KELICLOUD_SMOKE_ENDPOINT").unwrap_or_default();
    let mut session_token = std::env::var("KELICLOUD_SMOKE_SESSION_TOKEN").unwrap_or_default();
    let mut client_uuid = std::env::var("KELICLOUD_SMOKE_CLIENT_UUID").unwrap_or_default();
    let mut command = "printf 'kelicloud-terminal-smoke\\n'".to_string();
    let mut expect = "kelicloud-terminal-smoke".to_string();
    let mut timeout = 20_u64;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--endpoint" => endpoint = next_value(&mut args, "--endpoint")?,
            "--session-token" => session_token = next_value(&mut args, "--session-token")?,
            "--client" => client_uuid = next_value(&mut args, "--client")?,
            "--command" => command = next_value(&mut args, "--command")?,
            "--expect" => expect = next_value(&mut args, "--expect")?,
            "--timeout" => {
                let raw = next_value(&mut args, "--timeout")?;
                timeout = raw
                    .parse::<u64>()
                    .map_err(|_| "--timeout must be a whole number of seconds".to_string())?;
            }
            "--help" | "-h" => return Err("help requested".to_string()),
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    Ok(AdminTerminalSmokeRequest {
        endpoint,
        session_token,
        client_uuid,
        command,
        expect,
        timeout: Duration::from_secs(timeout.max(1)),
    })
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn print_usage() {
    eprintln!(
        "usage: admin-terminal-smoke --endpoint URL --session-token TOKEN --client UUID [--command CMD] [--expect TEXT] [--timeout SECONDS]"
    );
}
