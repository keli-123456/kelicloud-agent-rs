use std::path::PathBuf;

#[test]
fn canary_install_script_documents_real_host_stages() {
    let script = std::fs::read_to_string(canary_script_path()).unwrap();

    for expected in [
        "Real Linux host install canary",
        "--endpoint URL",
        "--auto-discovery KEY",
        "--install-version VERSION",
        "--rollback-command COMMAND",
        "install_agent",
        "verify_service",
        "restart_agent",
        "pin_or_upgrade_agent",
        "uninstall_agent",
        "run_rollback_command",
        "systemctl is-active",
        "journalctl -u kelicloud-agent-rs",
        "AGENT_ENDPOINT",
        "AGENT_AUTO_DISCOVERY_KEY",
        "kelicloud-agent-rs-linux",
    ] {
        assert!(script.contains(expected), "missing {expected}");
    }
}

#[cfg(unix)]
#[test]
fn canary_install_script_has_valid_bash_syntax_and_help() {
    let syntax = std::process::Command::new("bash")
        .arg("-n")
        .arg(canary_script_path())
        .output()
        .unwrap();
    assert!(
        syntax.status.success(),
        "{}",
        String::from_utf8_lossy(&syntax.stderr)
    );

    let help = std::process::Command::new("bash")
        .arg(canary_script_path())
        .arg("--help")
        .output()
        .unwrap();
    assert!(
        help.status.success(),
        "{}",
        String::from_utf8_lossy(&help.stderr)
    );
    let stdout = String::from_utf8_lossy(&help.stdout);
    assert!(stdout.contains("Real Linux host install canary"));
    assert!(stdout.contains("--rollback-command COMMAND"));
}

fn canary_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("canary-install.sh")
}
