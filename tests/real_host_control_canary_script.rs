use std::path::PathBuf;
use std::process::Command;

#[test]
fn real_host_control_canary_orchestrates_install_control_and_rollback() {
    let script = std::fs::read_to_string(real_host_control_canary_script_path()).unwrap();

    for expected in [
        "Real Linux host control-plane canary",
        "KELICLOUD_PANEL_COOKIE",
        "canary-install.sh",
        "live-panel-control-smoke.sh",
        "--keep-installed",
        "--tunnel-ktp-tcp-address \"$TUNNEL_KTP_TCP_ADDRESS\"",
        "--tunnel-ktp-tcp-auth-version \"$TUNNEL_KTP_TCP_AUTH_VERSION\"",
        "--tunnel-ktp-relay-batch-policy \"$TUNNEL_KTP_RELAY_BATCH_POLICY\"",
        "KELICLOUD_CANARY_TUNNEL_KTP_TCP_ADDRESS",
        "KELICLOUD_CANARY_TUNNEL_KTP_TCP_AUTH_VERSION",
        "KELICLOUD_CANARY_TUNNEL_KTP_RELAY_BATCH_POLICY",
        "INSTALL_VERSION=\"${KELICLOUD_CANARY_INSTALL_VERSION:-}\"",
        "--install-version VERSION   release tag to install/pin, default latest",
        "KTP TCP auth version",
        "KTP relay batch policy",
        "smoke: auto_discovery_registered",
        "uuid=",
        "parse_latest_registered_uuid",
        "run_control_smoke",
        "restore_old_on_exit",
        "systemctl enable --now",
        "komari-agent",
        "kelicloud-agent-rs",
    ] {
        assert!(script.contains(expected), "missing {expected}");
    }
}

#[test]
fn real_host_control_canary_keeps_panel_cookie_out_of_process_args() {
    let script = std::fs::read_to_string(real_host_control_canary_script_path()).unwrap();

    assert!(
        script.contains("KELICLOUD_PANEL_COOKIE=\"$COOKIE_HEADER\" bash"),
        "expected cookie to be passed through an environment variable"
    );
    assert!(
        !script.contains("args+=(--cookie \"$COOKIE_HEADER\")"),
        "raw cookie must not be appended to child process arguments"
    );
}

#[test]
fn real_host_control_canary_reports_success_as_passed_from_exit_trap() {
    let script = std::fs::read_to_string(real_host_control_canary_script_path()).unwrap();

    assert!(
        script.contains("local evidence_status=\"passed\""),
        "successful trap path should write a passed evidence result"
    );
    assert!(
        script.contains("if [[ \"$status\" -ne 0 ]]; then"),
        "failed trap path should preserve the non-zero exit status in evidence"
    );
    assert!(
        script.contains("write_evidence \"$evidence_status\""),
        "trap should write exactly one final evidence result"
    );
}

#[test]
fn real_host_control_canary_passes_panel_credentials_through_environment() {
    let script = std::fs::read_to_string(real_host_control_canary_script_path()).unwrap();

    for expected in [
        "KELICLOUD_PANEL_USERNAME",
        "KELICLOUD_PANEL_PASSWORD",
        "--username USERNAME",
        "--password PASSWORD",
        "PANEL_USERNAME",
        "PANEL_PASSWORD",
        "KELICLOUD_PANEL_USERNAME=\"$PANEL_USERNAME\"",
        "KELICLOUD_PANEL_PASSWORD=\"$PANEL_PASSWORD\"",
    ] {
        assert!(script.contains(expected), "missing {expected}");
    }

    assert!(
        !script.contains("args+=(--password \"$PANEL_PASSWORD\")"),
        "admin password must not be appended to child process arguments"
    );
}

#[test]
fn real_host_control_canary_waits_for_report_websocket_before_control_api() {
    let script = std::fs::read_to_string(real_host_control_canary_script_path()).unwrap();

    for expected in [
        "wait_for_rust_report_websocket",
        "smoke: report_websocket_connected",
        "smoke: report_sent",
        "systemctl restart \"${SERVICE_NAME}.service\"",
        "Rust report WebSocket connected and report sent.",
    ] {
        assert!(script.contains(expected), "missing {expected}");
    }

    let wait_pos = script.find("wait_for_rust_report_websocket").unwrap();
    let control_pos = script
        .find("bash \"${WORKDIR}/live-panel-control-smoke.sh\"")
        .unwrap();
    assert!(
        wait_pos < control_pos,
        "wrapper should wait for the Rust report websocket before calling live panel control APIs"
    );
}

#[test]
fn real_host_control_canary_has_valid_bash_syntax_when_bash_is_available() {
    let Some(bash) = find_bash() else {
        eprintln!("bash not available; skipping syntax check");
        return;
    };

    let output = Command::new(bash)
        .arg("-n")
        .arg(real_host_control_canary_script_path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "bash -n failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn real_host_control_canary_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("real-host-control-canary.sh")
}

fn find_bash() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(if cfg!(windows) { "bash.exe" } else { "bash" });
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    #[cfg(windows)]
    {
        for candidate in [
            r"C:\Program Files\Git\bin\bash.exe",
            r"C:\Program Files\Git\usr\bin\bash.exe",
        ] {
            let candidate = PathBuf::from(candidate);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}
