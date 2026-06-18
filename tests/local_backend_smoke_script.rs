use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
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
    assert!(script.contains("KELICLOUD_TUNNEL_ECHO_ROUNDS"));
    assert!(script.contains("KELICLOUD_TUNNEL_ECHO_CLIENTS"));
    assert!(script.contains("KELICLOUD_TUNNEL_ECHO_PROFILE"));
    assert!(script.contains("KELICLOUD_TUNNEL_ECHO_PAYLOAD_BYTES"));
    assert!(script.contains("require_positive_integer \"KELICLOUD_TUNNEL_ECHO_ROUNDS\""));
    assert!(script.contains("require_positive_integer \"KELICLOUD_TUNNEL_ECHO_CLIENTS\""));
    assert!(script.contains("for round in range(1, rounds + 1):"));
    assert!(script.contains("smoke: tunnel_relay_echo_succeeded"));
    assert!(script.contains("rounds=${KELICLOUD_TUNNEL_ECHO_ROUNDS}"));
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
fn local_backend_smoke_script_records_tunnel_echo_latency_evidence() {
    let script = std::fs::read_to_string(local_backend_smoke_script_path()).unwrap();

    assert!(script.contains("require_tunnel_echo_profile"));
    assert!(script.contains("\"fixed\" | \"rdp-like\""));
    assert!(script.contains("TUNNEL_ECHO_EVIDENCE_FILE"));
    assert!(script.contains("tunnel-echo.evidence.md"));
    assert!(script.contains("payload_for_round"));
    assert!(script.contains("client_count"));
    assert!(script.contains("client_worker"));
    assert!(script.contains("rtt_micros_p50"));
    assert!(script.contains("rtt_micros_p95"));
    assert!(script.contains("rtt_micros_p99"));
    assert!(script.contains("rtt_client_p95_spread_micros"));
    assert!(script.contains("total_payload_bytes"));
    assert!(script.contains("profile={profile}"));
    assert!(script.contains("smoke: tunnel_echo_evidence="));
}

#[test]
fn local_backend_smoke_script_prints_agent_logs_before_backend_logs() {
    let script = std::fs::read_to_string(local_backend_smoke_script_path()).unwrap();

    assert!(script.contains(r#"for file in "${AGENT_LOG}" "${HELPER_LOG}" "${BACKEND_LOG}"; do"#));
}

#[test]
fn local_backend_smoke_script_can_run_tunnel_relay_over_ktp_tcp() {
    let script = std::fs::read_to_string(local_backend_smoke_script_path()).unwrap();

    assert!(script.contains("KELICLOUD_SMOKE_KTP_TCP"));
    assert!(script.contains("KELICLOUD_SMOKE_KTP_TCP:-false"));
    assert!(script.contains("KTP_TCP_LISTEN"));
    assert!(script.contains("KOMARI_TUNNEL_KTP_TCP_ENABLED"));
    assert!(script.contains("KOMARI_TUNNEL_KTP_TCP_LISTEN"));
    assert!(script.contains("KOMARI_TUNNEL_KTP_TCP_ADDRESS"));
    assert!(script.contains("--tunnel-ktp-tcp-address"));
    assert!(script.contains("ktp-live-canary-evidence.sh"));
    assert!(script.contains("ktp-live-canary.evidence.md"));
    assert!(script.contains("smoke: ktp_live_canary_evidence="));
    assert!(script.contains("tunnel data diagnostics"));
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

    assert!(script.contains("BACKEND_START_TIMEOUT_SECONDS"));
    assert!(script.contains("BACKEND_START_TIMEOUT_SECONDS:-240"));
    assert!(script.contains(
        "wait_for_http \"${BACKEND_ENDPOINT}/ping\" \"${BACKEND_START_TIMEOUT_SECONDS}\""
    ));
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

#[test]
fn local_backend_smoke_script_generates_tunnel_echo_evidence() {
    let Some(bash) = find_bash() else {
        eprintln!("bash not available; skipping tunnel echo evidence test");
        return;
    };
    if !bash_has_python3(&bash) {
        eprintln!("python3 not available under bash; skipping tunnel echo evidence test");
        return;
    }

    let script = std::fs::read_to_string(local_backend_smoke_script_path()).unwrap();
    let sourced_script = script
        .strip_suffix("main \"$@\"\n")
        .or_else(|| script.strip_suffix("main \"$@\""))
        .expect("script should end with main invocation");
    let temp_dir = std::env::temp_dir().join(format!(
        "kelicloud-agent-rs-tunnel-echo-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let sourced_path = temp_dir.join("smoke-local-backend-functions.sh");
    let evidence_path = temp_dir.join("tunnel-echo.evidence.md");
    let agent_log_path = temp_dir.join("agent.log");
    std::fs::write(&sourced_path, sourced_script).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let listen_port = listener.local_addr().unwrap().port();
    let stop_echo = Arc::new(AtomicBool::new(false));
    let echo_stop = Arc::clone(&stop_echo);
    let echo = thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
        let mut accepted = 0;
        while !echo_stop.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buffer = vec![0; 65536];
                    let read = stream.read(&mut buffer).unwrap();
                    if read > 0 {
                        stream.write_all(b"echo:").unwrap();
                        stream.write_all(&buffer[..read]).unwrap();
                    }
                    accepted += 1;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => panic!("echo listener failed: {error}"),
            }
        }
        accepted
    });

    let output = Command::new(bash)
        .arg("-c")
        .arg(
            r#"
set -Eeuo pipefail
source "$1"
SMOKE_LOG_DIR="$2"
mkdir -p "${SMOKE_LOG_DIR}"
AGENT_LOG="$3"
: >"${AGENT_LOG}"
KELICLOUD_TUNNEL_ECHO_ROUNDS=3
KELICLOUD_TUNNEL_ECHO_CLIENTS=2
KELICLOUD_TUNNEL_ECHO_PROFILE=rdp-like
KELICLOUD_TUNNEL_ECHO_PAYLOAD_BYTES=1024
KELICLOUD_TUNNEL_ECHO_EVIDENCE="$4"
TUNNEL_RULE_ID=42
TUNNEL_LISTEN_PORT="$5"
verify_tunnel_relay_echo
"#,
        )
        .arg("bash")
        .arg(&sourced_path)
        .arg(&temp_dir)
        .arg(&agent_log_path)
        .arg(&evidence_path)
        .arg(listen_port.to_string())
        .output()
        .unwrap();
    stop_echo.store(true, Ordering::SeqCst);
    let accepted_connections = echo.join().unwrap();

    assert!(
        output.status.success(),
        "tunnel echo evidence helper failed after {accepted_connections} accepted connections:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        accepted_connections >= 6,
        "expected at least 6 accepted connections, got {accepted_connections}"
    );

    let evidence = std::fs::read_to_string(evidence_path).unwrap();
    assert!(evidence.contains("- profile: rdp-like"));
    assert!(evidence.contains("- rounds: 3"));
    assert!(evidence.contains("- clients: 2"));
    assert!(evidence.contains("- total_payload_bytes:"));
    assert!(evidence.contains("- rtt_micros_p95:"));
    assert!(evidence.contains("- rtt_client_p95_spread_micros:"));
    assert!(evidence.contains("| client | round | payload_bytes | rtt_micros |"));

    let agent_log = std::fs::read_to_string(agent_log_path).unwrap();
    assert!(agent_log.contains("smoke: tunnel_relay_echo_succeeded"));
    assert!(agent_log.contains("profile=rdp-like"));
    assert!(agent_log.contains("clients=2"));
    assert!(agent_log.contains("smoke: tunnel_echo_evidence="));
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

fn bash_has_python3(bash: &PathBuf) -> bool {
    Command::new(bash)
        .arg("-lc")
        .arg("python3 --version >/dev/null 2>&1")
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}
