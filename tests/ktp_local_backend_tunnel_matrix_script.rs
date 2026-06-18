use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn ktp_local_backend_tunnel_matrix_script_declares_contract() {
    let script = std::fs::read_to_string(script_path())
        .expect("local backend tunnel matrix script should be readable");

    assert!(script.contains("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENTS"));
    assert!(script.contains("1 2 4 8"));
    assert!(script.contains("KELICLOUD_SMOKE_KTP_TCP=true"));
    assert!(script.contains("KELICLOUD_TUNNEL_ECHO_CLIENTS"));
    assert!(script.contains("KELICLOUD_TUNNEL_ECHO_ROUNDS"));
    assert!(script.contains("KELICLOUD_TUNNEL_ECHO_PROFILE"));
    assert!(script.contains("KELICLOUD_TUNNEL_ECHO_PAYLOAD_BYTES"));
    assert!(script.contains("KOMARI_DB_NAME"));
    assert!(script.contains("SMOKE_AGENT_HOSTNAME"));
    assert!(script.contains("SMOKE_TUNNEL_GROUP"));
    assert!(script.contains("matrix_db_name"));
    assert!(script.contains("pick_free_tcp_port"));
    assert!(script.contains("BACKEND_LISTEN"));
    assert!(script.contains("BACKEND_ENDPOINT"));
    assert!(script.contains("KTP_LIVE_CANARY_MIN_MAX_BATCH_FRAMES"));
    assert!(script.contains("tunnel-echo.evidence.md"));
    assert!(script.contains("ktp-live-canary.evidence.md"));
    assert!(script.contains("matrix-summary.tsv"));
    assert!(script.contains("rtt_client_p95_spread_micros"));
    assert!(script.contains("socket_read_max_batch_frames"));
}

#[test]
fn ktp_local_backend_tunnel_matrix_script_has_valid_bash_syntax_when_bash_is_available() {
    let Some(bash) = find_bash() else {
        eprintln!("bash not available; skipping syntax check");
        return;
    };

    let output = Command::new(bash)
        .arg("-n")
        .arg(script_path())
        .output()
        .expect("bash -n should run");

    assert!(
        output.status.success(),
        "bash -n failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn ktp_local_backend_tunnel_matrix_script_dry_run_expands_clients() {
    let Some(bash) = find_bash() else {
        eprintln!("bash not available; skipping dry-run check");
        return;
    };

    let output = Command::new(bash)
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_DRY_RUN", "1")
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENTS", "1 4")
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ROUNDS", "8")
        .env(
            "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_LOG_DIR",
            "/tmp/ktp-tunnel-logs",
        )
        .env(
            "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_WORK_DIR",
            "/tmp/ktp-tunnel-work",
        )
        .arg(script_path())
        .output()
        .expect("tunnel matrix dry-run should run");

    assert!(
        output.status.success(),
        "tunnel matrix dry-run failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.matches("dry_run:").count(), 2);
    assert!(stdout.contains("clients=1"));
    assert!(stdout.contains("clients=4"));
    assert!(stdout.contains("KELICLOUD_TUNNEL_ECHO_CLIENTS=1"));
    assert!(stdout.contains("KELICLOUD_TUNNEL_ECHO_CLIENTS=4"));
    assert!(stdout.contains("SMOKE_LOG_DIR=/tmp/ktp-tunnel-logs/clients-1"));
    assert!(stdout.contains("SMOKE_LOG_DIR=/tmp/ktp-tunnel-logs/clients-4"));
    assert!(stdout.contains("SMOKE_WORK_DIR=/tmp/ktp-tunnel-work/clients-1"));
    assert!(stdout.contains("SMOKE_WORK_DIR=/tmp/ktp-tunnel-work/clients-4"));
    assert!(stdout.contains("KOMARI_DB_NAME=komari_tunnel_matrix_clients_1"));
    assert!(stdout.contains("KOMARI_DB_NAME=komari_tunnel_matrix_clients_4"));
    assert!(stdout.contains("SMOKE_AGENT_HOSTNAME=agent-rs-tunnel-matrix-c1"));
    assert!(stdout.contains("SMOKE_AGENT_HOSTNAME=agent-rs-tunnel-matrix-c4"));
    assert!(stdout.contains("SMOKE_TUNNEL_GROUP=agent-rs-tunnel-matrix-c1"));
    assert!(stdout.contains("SMOKE_TUNNEL_GROUP=agent-rs-tunnel-matrix-c4"));
    assert!(stdout.contains("BACKEND_LISTEN=auto"));
    assert!(stdout.contains("BACKEND_ENDPOINT=auto"));
}

#[test]
fn ktp_local_backend_tunnel_matrix_script_writes_summary_with_fake_smoke_on_linux() {
    if !cfg!(target_os = "linux") {
        eprintln!("linux-only fake smoke summary test skipped");
        return;
    }
    let Some(bash) = find_bash() else {
        eprintln!("bash not available; skipping fake smoke summary check");
        return;
    };

    let temp_dir = unique_temp_dir("ktp-local-backend-tunnel-matrix");
    let log_dir = temp_dir.join("logs");
    let work_dir = temp_dir.join("work");
    let summary_path = temp_dir.join("matrix-summary.tsv");
    let fake_smoke = temp_dir.join("fake-smoke.sh");
    std::fs::create_dir_all(&temp_dir).expect("temp dir should be created");
    std::fs::write(&fake_smoke, fake_smoke_script()).expect("fake smoke should be written");

    let output = Command::new(bash)
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENTS", "1 4")
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ROUNDS", "8")
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_PAYLOAD_BYTES", "8192")
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_LOG_DIR", &log_dir)
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_WORK_DIR", &work_dir)
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_SUMMARY", &summary_path)
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_SMOKE_SCRIPT", &fake_smoke)
        .arg(script_path())
        .output()
        .expect("tunnel matrix script should run with fake smoke");

    assert!(
        output.status.success(),
        "tunnel matrix script failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let summary = std::fs::read_to_string(&summary_path).expect("summary should be written");
    assert!(summary.contains(
        "clients\trounds\tprofile\tpayload_bytes\tstatus\tlog_dir\ttunnel_evidence_file\tktp_evidence_file\ttotal_payload_bytes\trtt_micros_p50\trtt_micros_p95\trtt_micros_p99\trtt_micros_max\trtt_client_p95_spread_micros\tsocket_read_batches\tsocket_read_frames\tsocket_read_max_batch_frames"
    ));
    assert!(summary.contains(&format!(
        "1\t8\trdp-like\t8192\tpass\t{}\t{}/tunnel-echo.evidence.md\t{}/ktp-live-canary.evidence.md\t9920\t100\t200\t300\t400\t0\t3\t40\t2",
        log_dir.join("clients-1").display(),
        log_dir.join("clients-1").display(),
        log_dir.join("clients-1").display()
    )));
    assert!(summary.contains(&format!(
        "4\t8\trdp-like\t8192\tpass\t{}\t{}/tunnel-echo.evidence.md\t{}/ktp-live-canary.evidence.md\t39680\t500\t600\t700\t800\t90\t12\t224\t11",
        log_dir.join("clients-4").display(),
        log_dir.join("clients-4").display(),
        log_dir.join("clients-4").display()
    )));
}

fn script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("ktp-local-backend-tunnel-matrix.sh")
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
}

fn fake_smoke_script() -> &'static str {
    r#"#!/usr/bin/env bash
set -euo pipefail
mkdir -p "${SMOKE_LOG_DIR}"
if [[ "${KELICLOUD_TUNNEL_ECHO_CLIENTS}" == "1" ]]; then
  total_payload_bytes=9920
  rtt_p50=100
  rtt_p95=200
  rtt_p99=300
  rtt_max=400
  spread=0
  socket_batches=3
  socket_frames=40
  socket_max_batch=2
else
  total_payload_bytes=39680
  rtt_p50=500
  rtt_p95=600
  rtt_p99=700
  rtt_max=800
  spread=90
  socket_batches=12
  socket_frames=224
  socket_max_batch=11
fi
cat >"${SMOKE_LOG_DIR}/tunnel-echo.evidence.md" <<EOF
# Tunnel Echo Evidence

- profile: ${KELICLOUD_TUNNEL_ECHO_PROFILE}
- rounds: ${KELICLOUD_TUNNEL_ECHO_ROUNDS}
- clients: ${KELICLOUD_TUNNEL_ECHO_CLIENTS}
- total_payload_bytes: ${total_payload_bytes}
- rtt_micros_p50: ${rtt_p50}
- rtt_micros_p95: ${rtt_p95}
- rtt_micros_p99: ${rtt_p99}
- rtt_micros_max: ${rtt_max}
- rtt_client_p95_spread_micros: ${spread}
EOF
cat >"${SMOKE_LOG_DIR}/ktp-live-canary.evidence.md" <<EOF
# KTP Live Canary Evidence

## Positive Fields

- \`socket_read_batches\`: \`${socket_batches}\`
- \`socket_read_frames\`: \`${socket_frames}\`

## Batch Thresholds

- \`socket_read_max_batch_frames\`: \`${socket_max_batch}\`
EOF
"#
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
