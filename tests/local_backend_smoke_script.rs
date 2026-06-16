use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn local_backend_smoke_script_orchestrates_real_backend_controls() {
    let script = std::fs::read_to_string(local_backend_smoke_script_path()).unwrap();

    assert!(script.contains("KELICLOUD_BACKEND_REPO"));
    assert!(script.contains("KELICLOUD_BACKEND_REF"));
    assert!(script.contains("scripts/prepare-frontend.sh"));
    assert!(script.contains("/api/login"));
    assert!(script.contains("/api/admin/settings/"));
    assert!(script.contains("AUTO_DISCOVERY_KEY"));
    assert!(script.contains("--auto-discovery"));
    assert!(script.contains("HOSTNAME=\"${SMOKE_AGENT_HOSTNAME}\""));
    assert!(script.contains("--info-report-interval 0"));
    assert!(script.contains("/api/admin/client/list"));
    assert!(script.contains("/api/admin/client/${CLIENT_UUID}/token"));
    assert!(script.contains("/api/admin/client/${CLIENT_UUID}/edit"));
    assert!(script.contains("/api/admin/task/exec"));
    assert!(script.contains("/api/admin/ping/add"));
    assert!(script.contains("/api/admin/settings/system"));
    assert!(script.contains("/api/admin/tunnels"));
    assert!(script.contains("rotate_auto_discovery_token"));
    assert!(script.contains("wait_for_auto_discovery_recovery"));
    assert!(script.contains("restart_agent_after_token_recovery"));
    assert!(script.contains("resolve_auto_discovery_client"));
    assert!(script.contains("AGENT_TUNNEL_DATA_ENABLED=true"));
    assert!(script.contains("create_tunnel_rule"));
    assert!(script.contains("verify_tunnel_relay_echo"));
    assert!(script.contains("smoke: tunnel_relay_echo_succeeded"));
    assert!(script.contains("wait_for_log_count"));
    assert!(script.contains("\"smoke: auto_discovery_registered\" 2"));
    assert!(script.contains("\"smoke: token_recovered\" 1"));
    assert!(!script.contains("--token \"${AGENT_TOKEN}\""));
    assert!(script.contains("admin-terminal-smoke"));
    assert!(script.contains("smoke-summary --require-pass"));
    assert!(script.contains("wait_for_log"));
    assert!(script.contains("smoke: ping_result_uploaded"));
    assert!(script.contains("smoke: task_result_uploaded"));
    assert!(script.contains("smoke: terminal_session_started"));
    assert!(script.contains("smoke: cn_connectivity_config_received"));
    assert!(script.contains("live smoke duration reached"));
    assert!(script.contains(": >\"${AGENT_LOG}\""));
    assert!(script.contains(">>\"${AGENT_LOG}\" 2>&1 &"));
    assert!(script.contains("trap on_error ERR"));
    assert!(script.contains("CURRENT_STAGE"));
    assert!(script.contains("::error title=Local backend smoke::"));
    assert!(script.contains("sys.argv[1]"));
    assert!(!script.contains("os.environ[\"ADMIN_USERNAME\"]"));
}

#[test]
fn local_backend_smoke_script_prints_agent_logs_before_backend_logs() {
    let script = std::fs::read_to_string(local_backend_smoke_script_path()).unwrap();

    assert!(script.contains(r#"for file in "${AGENT_LOG}" "${HELPER_LOG}" "${BACKEND_LOG}"; do"#));
}

#[test]
fn local_backend_smoke_script_surfaces_terminal_helper_failures() {
    let script = std::fs::read_to_string(local_backend_smoke_script_path()).unwrap();

    assert!(script.contains("admin-terminal-smoke failed"));
    assert!(script.contains("tail -n 80 \"${HELPER_LOG}\""));
    assert!(script.contains("admin-terminal-smoke failed${details}$(log_tail_for_error)"));
}

#[test]
fn local_backend_smoke_script_requires_node_only_when_preparing_frontend() {
    let script = std::fs::read_to_string(local_backend_smoke_script_path()).unwrap();

    assert!(script.contains(
        "if [[ \"${KELICLOUD_PREPARE_FRONTEND}\" == \"true\" ]]; then\n        require_command node\n        require_command npm\n    fi"
    ));
}

#[test]
fn local_backend_smoke_script_retries_admin_login_until_database_is_ready() {
    let script = std::fs::read_to_string(local_backend_smoke_script_path()).unwrap();

    assert!(script.contains("timed out waiting for admin login"));
    assert!(script.contains("sleep 1"));
}

#[test]
fn local_backend_smoke_script_surfaces_backend_startup_logs() {
    let script = std::fs::read_to_string(local_backend_smoke_script_path()).unwrap();

    assert!(script.contains("timed out waiting for ${url}$(log_tail_for_error)"));
}

#[test]
fn local_backend_smoke_script_keeps_recovered_client_after_agent_restart() {
    let script = std::fs::read_to_string(local_backend_smoke_script_path()).unwrap();
    let start = script
        .find("restart_agent_after_token_recovery()")
        .expect("restart helper should exist");
    let tail = &script[start..];
    let end = tail
        .find("\n}\n\nset_client_tunnel_group")
        .expect("restart helper should end before tunnel group helper");
    let body = &tail[..end];

    assert!(body.contains("wait_for_log_count"));
    assert!(!body.contains("resolve_auto_discovery_client"));
}

#[test]
fn local_backend_smoke_script_has_valid_bash_syntax_when_bash_is_available() {
    let Some(bash) = find_bash() else {
        eprintln!("bash not available; skipping syntax check");
        return;
    };
    let output = Command::new(bash)
        .arg("-n")
        .arg(local_backend_smoke_script_path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "bash -n failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn local_backend_smoke_script_reads_latest_auto_discovery_uuid_from_agent_log() {
    let Some(bash) = find_bash() else {
        eprintln!("bash not available; skipping latest auto-discovery UUID test");
        return;
    };

    let script = std::fs::read_to_string(local_backend_smoke_script_path()).unwrap();
    let sourced_script = script
        .strip_suffix("main \"$@\"\n")
        .or_else(|| script.strip_suffix("main \"$@\""))
        .expect("script should end with main invocation");
    let temp_dir = std::env::temp_dir().join(format!(
        "kelicloud-agent-rs-smoke-script-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let sourced_path = temp_dir.join("smoke-local-backend-functions.sh");
    let agent_log_path = temp_dir.join("agent.log");
    std::fs::write(&sourced_path, sourced_script).unwrap();
    std::fs::write(
        &agent_log_path,
        "noise\nsmoke: auto_discovery_registered uuid=old-uuid\nsmoke: token_recovered operation=upload_basic_info\nsmoke: auto_discovery_registered uuid=new-uuid\n",
    )
    .unwrap();

    let output = Command::new(bash)
        .arg("-c")
        .arg(r#"source "$1"; AGENT_LOG="$2"; latest_auto_discovery_registered_uuid"#)
        .arg("bash")
        .arg(&sourced_path)
        .arg(&agent_log_path)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "latest UUID helper failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "new-uuid");
}

fn local_backend_smoke_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("smoke-local-backend.sh")
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
