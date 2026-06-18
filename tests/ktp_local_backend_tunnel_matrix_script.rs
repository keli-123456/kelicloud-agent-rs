use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn ktp_local_backend_tunnel_matrix_script_declares_contract() {
    let script = std::fs::read_to_string(script_path())
        .expect("local backend tunnel matrix script should be readable");

    assert!(script.contains("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENTS"));
    assert!(script.contains("1 2 4 8"));
    assert!(script.contains("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_RELAY_BATCH_POLICIES"));
    assert!(script.contains("AGENT_TUNNEL_KTP_RELAY_BATCH_POLICY"));
    assert!(script.contains("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ADAPTIVE_HIGH_SESSIONS"));
    assert!(script.contains("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ADAPTIVE_ELEVATED_DWELL_US"));
    assert!(script.contains("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ADAPTIVE_SEVERE_DWELL_US"));
    assert!(script.contains("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ADAPTIVE_ELEVATED_CAP"));
    assert!(script.contains("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ADAPTIVE_SEVERE_CAP"));
    assert!(script.contains("AGENT_TUNNEL_KTP_RELAY_ADAPTIVE_HIGH_SESSIONS"));
    assert!(script.contains("AGENT_TUNNEL_KTP_RELAY_ADAPTIVE_ELEVATED_DWELL_US"));
    assert!(script.contains("AGENT_TUNNEL_KTP_RELAY_ADAPTIVE_SEVERE_DWELL_US"));
    assert!(script.contains("AGENT_TUNNEL_KTP_RELAY_ADAPTIVE_ELEVATED_CAP"));
    assert!(script.contains("AGENT_TUNNEL_KTP_RELAY_ADAPTIVE_SEVERE_CAP"));
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
    assert!(script.contains("KTP_LIVE_CANARY_MIN_MAX_WRITE_BATCH_FRAMES"));
    assert!(script.contains("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MIN_MAX_WRITE_BATCH_FRAMES"));
    assert!(script.contains("tunnel-echo.evidence.md"));
    assert!(script.contains("ktp-live-canary.evidence.md"));
    assert!(script.contains("matrix-summary.tsv"));
    assert!(script.contains("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENT_TIMEOUT_SECONDS"));
    assert!(script.contains("elapsed_millis"));
    assert!(script.contains("echo_elapsed_micros"));
    assert!(script.contains("timeout"));
    assert!(script.contains("rtt_client_p95_spread_micros"));
    assert!(script.contains("socket_read_max_batch_frames"));
    assert!(script.contains("socket_write_max_batch_frames"));
    assert!(script.contains("socket_write_batch_limit_max"));
    assert!(script.contains("socket_write_batch_limit_min"));
    assert!(script.contains("socket_write_batch_limit_last"));
    assert!(script.contains("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MAX_RTT_P95_MICROS"));
    assert!(script.contains("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MAX_CLIENT_P95_SPREAD_MICROS"));
    assert!(script.contains("performance_gate_failures"));
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
            "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENT_TIMEOUT_SECONDS",
            "300",
        )
        .env(
            "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MIN_MAX_WRITE_BATCH_FRAMES",
            "2",
        )
        .env(
            "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ADAPTIVE_HIGH_SESSIONS",
            "4",
        )
        .env(
            "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ADAPTIVE_ELEVATED_DWELL_US",
            "40000",
        )
        .env(
            "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ADAPTIVE_SEVERE_DWELL_US",
            "120000",
        )
        .env(
            "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ADAPTIVE_ELEVATED_CAP",
            "24",
        )
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ADAPTIVE_SEVERE_CAP", "6")
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
    assert!(stdout.contains("SMOKE_LOG_DIR=/tmp/ktp-tunnel-logs/fixed/clients-1"));
    assert!(stdout.contains("SMOKE_LOG_DIR=/tmp/ktp-tunnel-logs/fixed/clients-4"));
    assert!(stdout.contains("SMOKE_WORK_DIR=/tmp/ktp-tunnel-work/fixed/clients-1"));
    assert!(stdout.contains("SMOKE_WORK_DIR=/tmp/ktp-tunnel-work/fixed/clients-4"));
    assert!(stdout.contains("KOMARI_DB_NAME=komari_tunnel_matrix_fixed_clients_1"));
    assert!(stdout.contains("KOMARI_DB_NAME=komari_tunnel_matrix_fixed_clients_4"));
    assert!(stdout.contains("SMOKE_AGENT_HOSTNAME=agent-rs-tunnel-matrix-fixed-c1"));
    assert!(stdout.contains("SMOKE_AGENT_HOSTNAME=agent-rs-tunnel-matrix-fixed-c4"));
    assert!(stdout.contains("SMOKE_TUNNEL_GROUP=agent-rs-tunnel-matrix-fixed-c1"));
    assert!(stdout.contains("SMOKE_TUNNEL_GROUP=agent-rs-tunnel-matrix-fixed-c4"));
    assert!(stdout.contains("BACKEND_LISTEN=auto"));
    assert!(stdout.contains("BACKEND_ENDPOINT=auto"));
    assert!(stdout.contains("KTP_LIVE_CANARY_MIN_MAX_WRITE_BATCH_FRAMES=2"));
    assert!(stdout.contains("AGENT_TUNNEL_KTP_RELAY_ADAPTIVE_HIGH_SESSIONS=4"));
    assert!(stdout.contains("AGENT_TUNNEL_KTP_RELAY_ADAPTIVE_ELEVATED_DWELL_US=40000"));
    assert!(stdout.contains("AGENT_TUNNEL_KTP_RELAY_ADAPTIVE_SEVERE_DWELL_US=120000"));
    assert!(stdout.contains("AGENT_TUNNEL_KTP_RELAY_ADAPTIVE_ELEVATED_CAP=24"));
    assert!(stdout.contains("AGENT_TUNNEL_KTP_RELAY_ADAPTIVE_SEVERE_CAP=6"));
    assert!(stdout.contains("CLIENT_TIMEOUT_SECONDS=300"));
}

#[test]
fn ktp_local_backend_tunnel_matrix_script_dry_run_expands_policies_and_clients() {
    let Some(bash) = find_bash() else {
        eprintln!("bash not available; skipping policy dry-run check");
        return;
    };

    let output = Command::new(bash)
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_DRY_RUN", "1")
        .env(
            "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_RELAY_BATCH_POLICIES",
            "fixed adaptive",
        )
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENTS", "1 4")
        .env(
            "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_LOG_DIR",
            "/tmp/ktp-tunnel-logs",
        )
        .arg(script_path())
        .output()
        .expect("tunnel matrix policy dry-run should run");

    assert!(
        output.status.success(),
        "tunnel matrix policy dry-run failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.matches("dry_run:").count(), 4);
    assert!(stdout.contains("relay_batch_policy=fixed clients=1"));
    assert!(stdout.contains("relay_batch_policy=fixed clients=4"));
    assert!(stdout.contains("relay_batch_policy=adaptive clients=1"));
    assert!(stdout.contains("relay_batch_policy=adaptive clients=4"));
    assert!(stdout.contains("AGENT_TUNNEL_KTP_RELAY_BATCH_POLICY=fixed"));
    assert!(stdout.contains("AGENT_TUNNEL_KTP_RELAY_BATCH_POLICY=adaptive"));
    assert!(stdout.contains("SMOKE_LOG_DIR=/tmp/ktp-tunnel-logs/fixed/clients-1"));
    assert!(stdout.contains("SMOKE_LOG_DIR=/tmp/ktp-tunnel-logs/adaptive/clients-4"));
    assert!(stdout.contains("KOMARI_DB_NAME=komari_tunnel_matrix_fixed_clients_1"));
    assert!(stdout.contains("KOMARI_DB_NAME=komari_tunnel_matrix_adaptive_clients_4"));
}

#[test]
fn ktp_local_backend_tunnel_matrix_script_truncates_long_database_names_for_mysql() {
    let Some(bash) = find_bash() else {
        eprintln!("bash not available; skipping long database name dry-run check");
        return;
    };

    let output = Command::new(bash)
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_DRY_RUN", "1")
        .env(
            "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_DB_PREFIX",
            "komari_tunnel_matrix_high_19cb4ce_20260618104155_extra_long_release_host_prefix",
        )
        .env(
            "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_RELAY_BATCH_POLICIES",
            "adaptive",
        )
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENTS", "4")
        .arg(script_path())
        .output()
        .expect("long database name dry-run should run");

    assert!(
        output.status.success(),
        "long database name dry-run failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let db_name = stdout
        .split_whitespace()
        .find_map(|field| field.strip_prefix("KOMARI_DB_NAME="))
        .expect("dry-run should include KOMARI_DB_NAME");

    assert!(
        db_name.len() <= 64,
        "database name should fit MySQL's 64 byte identifier limit: {db_name} ({})",
        db_name.len()
    );
    assert!(
        db_name.ends_with("_adaptive_clients_4"),
        "database name should preserve matrix identity suffix: {db_name}"
    );
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
        .env(
            "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ADAPTIVE_HIGH_SESSIONS",
            "4",
        )
        .env(
            "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ADAPTIVE_ELEVATED_DWELL_US",
            "40000",
        )
        .env(
            "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ADAPTIVE_SEVERE_DWELL_US",
            "120000",
        )
        .env(
            "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ADAPTIVE_ELEVATED_CAP",
            "24",
        )
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ADAPTIVE_SEVERE_CAP", "6")
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
        "relay_batch_policy\tclients\trelay_adaptive_high_sessions\trelay_adaptive_elevated_dwell_us\trelay_adaptive_severe_dwell_us\trelay_adaptive_elevated_cap\trelay_adaptive_severe_cap\trounds\tprofile\tpayload_bytes\tstatus\telapsed_millis\tlog_dir\ttunnel_evidence_file\tktp_evidence_file\ttotal_payload_bytes\techo_elapsed_micros\trtt_micros_p50\trtt_micros_p95\trtt_micros_p99\trtt_micros_max\trtt_client_p95_spread_micros\tsocket_read_batches\tsocket_read_frames\tsocket_read_max_batch_frames\tsocket_write_batches\tsocket_write_frames\tsocket_write_max_batch_frames\tsocket_write_batch_limit_max\tsocket_write_batch_limit_min\tsocket_write_batch_limit_last"
    ));
    assert_summary_row_with_adaptive(
        &summary,
        "fixed",
        "1",
        &["4", "40000", "120000", "24", "6"],
        &[
            "8",
            "rdp-like",
            "8192",
            "pass",
            &log_dir
                .join("fixed")
                .join("clients-1")
                .display()
                .to_string(),
            &format!(
                "{}/tunnel-echo.evidence.md",
                log_dir.join("fixed").join("clients-1").display()
            ),
            &format!(
                "{}/ktp-live-canary.evidence.md",
                log_dir.join("fixed").join("clients-1").display()
            ),
            "9920",
            "100000",
            "100",
            "200",
            "300",
            "400",
            "0",
            "3",
            "40",
            "2",
            "2",
            "40",
            "5",
            "64",
            "64",
            "64",
        ],
    );
    assert_summary_row_with_adaptive(
        &summary,
        "fixed",
        "4",
        &["4", "40000", "120000", "24", "6"],
        &[
            "8",
            "rdp-like",
            "8192",
            "pass",
            &log_dir
                .join("fixed")
                .join("clients-4")
                .display()
                .to_string(),
            &format!(
                "{}/tunnel-echo.evidence.md",
                log_dir.join("fixed").join("clients-4").display()
            ),
            &format!(
                "{}/ktp-live-canary.evidence.md",
                log_dir.join("fixed").join("clients-4").display()
            ),
            "39680",
            "200000",
            "500",
            "600",
            "700",
            "800",
            "90",
            "12",
            "224",
            "11",
            "10",
            "236",
            "12",
            "64",
            "16",
            "16",
        ],
    );
}

#[test]
fn ktp_local_backend_tunnel_matrix_script_marks_timed_out_client_run_on_linux() {
    if !cfg!(target_os = "linux") {
        eprintln!("linux-only timeout summary test skipped");
        return;
    }
    let Some(bash) = find_bash() else {
        eprintln!("bash not available; skipping timeout summary check");
        return;
    };

    let temp_dir = unique_temp_dir("ktp-local-backend-tunnel-matrix-timeout");
    let log_dir = temp_dir.join("logs");
    let summary_path = temp_dir.join("matrix-summary.tsv");
    let slow_smoke = temp_dir.join("slow-smoke.sh");
    std::fs::create_dir_all(&temp_dir).expect("temp dir should be created");
    std::fs::write(
        &slow_smoke,
        r#"#!/usr/bin/env bash
set -euo pipefail
mkdir -p "${SMOKE_LOG_DIR}"
sleep 5
"#,
    )
    .expect("slow smoke should be written");

    let output = Command::new(bash)
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENTS", "1")
        .env(
            "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENT_TIMEOUT_SECONDS",
            "1",
        )
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_LOG_DIR", &log_dir)
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_SUMMARY", &summary_path)
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_SMOKE_SCRIPT", &slow_smoke)
        .arg(script_path())
        .output()
        .expect("tunnel matrix timeout script should run");

    assert!(
        !output.status.success(),
        "timeout run should fail:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let summary = std::fs::read_to_string(&summary_path).expect("summary should be written");
    assert_summary_row(
        &summary,
        "fixed",
        "1",
        &[
            "8",
            "rdp-like",
            "8192",
            "timeout",
            &log_dir
                .join("fixed")
                .join("clients-1")
                .display()
                .to_string(),
            "-",
            "-",
            "-",
            "-",
            "-",
            "-",
            "-",
            "-",
            "-",
            "-",
            "-",
            "-",
            "-",
            "-",
            "-",
            "-",
            "-",
            "-",
        ],
    );
}

#[test]
fn ktp_local_backend_tunnel_matrix_script_latency_gate_fails_after_writing_summary_on_linux() {
    if !cfg!(target_os = "linux") {
        eprintln!("linux-only latency gate summary test skipped");
        return;
    }
    let Some(bash) = find_bash() else {
        eprintln!("bash not available; skipping latency gate summary check");
        return;
    };

    let temp_dir = unique_temp_dir("ktp-local-backend-tunnel-matrix-latency-gate");
    let log_dir = temp_dir.join("logs");
    let summary_path = temp_dir.join("matrix-summary.tsv");
    let fake_smoke = temp_dir.join("fake-smoke.sh");
    std::fs::create_dir_all(&temp_dir).expect("temp dir should be created");
    std::fs::write(&fake_smoke, fake_smoke_script()).expect("fake smoke should be written");

    let output = Command::new(bash)
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENTS", "1 4")
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_LOG_DIR", &log_dir)
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_SUMMARY", &summary_path)
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_SMOKE_SCRIPT", &fake_smoke)
        .env("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MAX_RTT_P95_MICROS", "500")
        .env(
            "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MAX_CLIENT_P95_SPREAD_MICROS",
            "80",
        )
        .arg(script_path())
        .output()
        .expect("tunnel matrix latency gate script should run");

    assert_eq!(
        output.status.code(),
        Some(3),
        "latency gate should exit 3:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("rtt_micros_p95 600 exceeds max 500 for policy=fixed clients=4"));
    assert!(stderr
        .contains("rtt_client_p95_spread_micros 90 exceeds max 80 for policy=fixed clients=4"));
    let summary = std::fs::read_to_string(&summary_path).expect("summary should be written");
    assert_summary_row(
        &summary,
        "fixed",
        "1",
        &[
            "8",
            "rdp-like",
            "8192",
            "pass",
            &log_dir
                .join("fixed")
                .join("clients-1")
                .display()
                .to_string(),
            &format!(
                "{}/tunnel-echo.evidence.md",
                log_dir.join("fixed").join("clients-1").display()
            ),
            &format!(
                "{}/ktp-live-canary.evidence.md",
                log_dir.join("fixed").join("clients-1").display()
            ),
            "9920",
            "100000",
            "100",
            "200",
            "300",
            "400",
            "0",
            "3",
            "40",
            "2",
            "2",
            "40",
            "5",
            "64",
            "64",
            "64",
        ],
    );
    assert_summary_row(
        &summary,
        "fixed",
        "4",
        &[
            "8",
            "rdp-like",
            "8192",
            "pass",
            &log_dir
                .join("fixed")
                .join("clients-4")
                .display()
                .to_string(),
            &format!(
                "{}/tunnel-echo.evidence.md",
                log_dir.join("fixed").join("clients-4").display()
            ),
            &format!(
                "{}/ktp-live-canary.evidence.md",
                log_dir.join("fixed").join("clients-4").display()
            ),
            "39680",
            "200000",
            "500",
            "600",
            "700",
            "800",
            "90",
            "12",
            "224",
            "11",
            "10",
            "236",
            "12",
            "64",
            "16",
            "16",
        ],
    );
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

fn assert_summary_row(
    summary: &str,
    relay_batch_policy: &str,
    clients: &str,
    expected_after_elapsed: &[&str],
) {
    assert_summary_row_with_adaptive(
        summary,
        relay_batch_policy,
        clients,
        &["8", "50000", "250000", "16", "8"],
        expected_after_elapsed,
    );
}

fn assert_summary_row_with_adaptive(
    summary: &str,
    relay_batch_policy: &str,
    clients: &str,
    expected_adaptive: &[&str],
    expected_after_elapsed: &[&str],
) {
    let row = summary
        .lines()
        .find(|line| line.starts_with(&format!("{relay_batch_policy}\t{clients}\t")))
        .unwrap_or_else(|| {
            panic!(
                "summary row for policy={relay_batch_policy} clients={clients} should exist:\n{summary}"
            )
        });
    let columns = row.split('\t').collect::<Vec<_>>();
    assert_eq!(columns[0], relay_batch_policy);
    assert_eq!(columns[1], clients);
    assert_eq!(&columns[2..7], expected_adaptive);
    assert!(
        columns[11].parse::<u64>().is_ok(),
        "elapsed_millis should be an unsigned integer in row: {row}"
    );
    assert_eq!(&columns[7..11], &expected_after_elapsed[0..4]);
    assert_eq!(&columns[12..], &expected_after_elapsed[4..]);
}

fn fake_smoke_script() -> &'static str {
    r#"#!/usr/bin/env bash
set -euo pipefail
mkdir -p "${SMOKE_LOG_DIR}"
if [[ "${KELICLOUD_TUNNEL_ECHO_CLIENTS}" == "1" ]]; then
  total_payload_bytes=9920
  echo_elapsed_micros=100000
  rtt_p50=100
  rtt_p95=200
  rtt_p99=300
  rtt_max=400
  spread=0
  socket_batches=3
  socket_frames=40
  socket_max_batch=2
  socket_write_batches=2
  socket_write_frames=40
  socket_write_max_batch=5
  socket_write_batch_limit_max=64
  socket_write_batch_limit_min=64
  socket_write_batch_limit_last=64
else
  total_payload_bytes=39680
  echo_elapsed_micros=200000
  rtt_p50=500
  rtt_p95=600
  rtt_p99=700
  rtt_max=800
  spread=90
  socket_batches=12
  socket_frames=224
  socket_max_batch=11
  socket_write_batches=10
  socket_write_frames=236
  socket_write_max_batch=12
  socket_write_batch_limit_max=64
  socket_write_batch_limit_min=16
  socket_write_batch_limit_last=16
fi
cat >"${SMOKE_LOG_DIR}/tunnel-echo.evidence.md" <<EOF
# Tunnel Echo Evidence

- profile: ${KELICLOUD_TUNNEL_ECHO_PROFILE}
- rounds: ${KELICLOUD_TUNNEL_ECHO_ROUNDS}
- clients: ${KELICLOUD_TUNNEL_ECHO_CLIENTS}
- total_payload_bytes: ${total_payload_bytes}
- echo_elapsed_micros: ${echo_elapsed_micros}
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
- \`socket_write_batches\`: \`${socket_write_batches}\`
- \`socket_write_frames\`: \`${socket_write_frames}\`
- \`socket_write_max_batch_frames\`: \`${socket_write_max_batch}\`
- \`socket_write_batch_limit_max\`: \`${socket_write_batch_limit_max}\`
- \`socket_write_batch_limit_min\`: \`${socket_write_batch_limit_min}\`
- \`socket_write_batch_limit_last\`: \`${socket_write_batch_limit_last}\`

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
