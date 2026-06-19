use std::path::PathBuf;

#[test]
fn canary_install_script_documents_real_host_stages() {
    let script = std::fs::read_to_string(canary_script_path()).unwrap();

    for expected in [
        "Real Linux host install canary",
        "--endpoint URL",
        "--auto-discovery KEY",
        "--install-version VERSION",
        "--tunnel-ktp-tcp-address ADDRESS",
        "--tunnel-ktp-tcp-auth-version VERSION",
        "--tunnel-ktp-relay-batch-policy POLICY",
        "--rollback-command COMMAND",
        "--rollback-service-name NAME",
        "--skip-rollback-service-check",
        "--evidence-file PATH",
        "install_agent",
        "verify_service",
        "restart_agent",
        "pin_or_upgrade_agent",
        "verify_installed_version",
        "uninstall_agent",
        "run_rollback_command",
        "verify_rollback_service",
        "systemctl is-active",
        "journalctl -u kelicloud-agent-rs",
        "journalctl -u ${ROLLBACK_SERVICE_NAME}",
        "KELICLOUD_ROLLBACK_SERVICE_NAME",
        "KELICLOUD_CANARY_EVIDENCE_FILE",
        "write_evidence",
        "printf '%s\\n' '- Result:",
        "Operator Notes",
        "Panel online and metrics",
        "kelicloud-agent",
        "AGENT_ENDPOINT",
        "AGENT_AUTO_DISCOVERY_KEY",
        "AGENT_TUNNEL_KTP_TCP_ADDRESS",
        "AGENT_TUNNEL_KTP_TCP_AUTH_VERSION",
        "AGENT_TUNNEL_KTP_RELAY_BATCH_POLICY",
        "--tunnel-ktp-tcp-auth-version \"$TUNNEL_KTP_TCP_AUTH_VERSION\"",
        "--tunnel-ktp-relay-batch-policy \"$TUNNEL_KTP_RELAY_BATCH_POLICY\"",
        "KTP TCP auth version",
        "KTP relay batch policy",
        "kelicloud-agent-rs-linux",
        "\"$BIN_PATH\" --version",
        "expected_version=\"${INSTALL_VERSION#v}\"",
        "installed version mismatch",
        "- Installed binary version result:",
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
    assert!(stdout.contains("--rollback-service-name NAME"));
    assert!(stdout.contains("--tunnel-ktp-tcp-auth-version VERSION"));
    assert!(stdout.contains("--tunnel-ktp-relay-batch-policy POLICY"));
    assert!(stdout.contains("--evidence-file PATH"));
}

#[test]
fn canary_install_script_collects_ktp_live_evidence_when_tunnel_enabled() {
    let script = std::fs::read_to_string(canary_script_path()).unwrap();

    for expected in [
        "KTP_EVIDENCE_SCRIPT_URL",
        "KTP_LIVE_CANARY_EVIDENCE_FILE",
        "KTP_LIVE_CANARY_TUNNEL_ECHO_EVIDENCE_FILE",
        "download_ktp_evidence_script",
        "collect_ktp_live_canary_evidence",
        "ktp-live-canary-evidence.sh",
        "KTP_LIVE_CANARY_AUTH_VERSION=\"${TUNNEL_KTP_TCP_AUTH_VERSION:-v1}\"",
        "--tunnel-echo-evidence-file \"$KTP_LIVE_CANARY_TUNNEL_ECHO_EVIDENCE_FILE\"",
        "--since \"@${KTP_EVIDENCE_SINCE_EPOCH}\"",
        "if [[ -z \"$TUNNEL_KTP_TCP_ADDRESS\" ]]",
        "smoke: ktp_live_canary_evidence=",
        "- KTP live canary evidence:",
        "- KTP tunnel echo evidence:",
        "- KTP live canary result:",
    ] {
        assert!(script.contains(expected), "missing {expected}");
    }

    assert!(
        script.find("observe_panel_window") < script.find("collect_ktp_live_canary_evidence"),
        "KTP evidence should be collected after the operator traffic window"
    );
}

fn canary_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("canary-install.sh")
}
