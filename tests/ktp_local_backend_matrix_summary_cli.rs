use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn ktp_local_backend_matrix_summary_reports_carrier_comparison() {
    let summary_path = write_temp_summary(
        "ktp-local-backend-matrix-summary-pass",
        r#"carrier	ktp_tcp	ktp_crypto	status	log_dir	summary_file	ktp_evidence_file	tunnel_evidence_file	tunnel_profile	tunnel_clients	tunnel_rounds	tunnel_total_payload_bytes	rtt_micros_p50	rtt_micros_p95	rtt_micros_p99	rtt_micros_max	rtt_client_p95_spread_micros
websocket	false	-	pass	logs/websocket	logs/websocket/agent.summary.md	-	logs/websocket/tunnel-echo.evidence.md	fixed	1	1	64	1000	1100	1200	1300	0
ktp_tcp	true	ktp_aead	pass	logs/ktp_tcp	logs/ktp_tcp/agent.summary.md	logs/ktp_tcp/ktp-live-canary.evidence.md	logs/ktp_tcp/tunnel-echo.evidence.md	rdp-like	4	8	39680	5958	26909	30755	30755	16954
"#,
    );

    let output = Command::new(summary_exe())
        .arg(&summary_path)
        .output()
        .expect("ktp-local-backend-matrix-summary should run");

    assert!(
        output.status.success(),
        "summary failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout
        .contains("ktp_local_backend_matrix_summary rows=2 pass=2 fail=0 timeout=0 status=pass"));
    assert!(stdout.contains(
        "carrier=websocket ktp_tcp=false ktp_crypto=- status=pass summary_file=logs/websocket/agent.summary.md ktp_evidence_file=-"
    ));
    assert!(stdout.contains(
        "carrier=ktp_tcp ktp_tcp=true ktp_crypto=ktp_aead status=pass summary_file=logs/ktp_tcp/agent.summary.md ktp_evidence_file=logs/ktp_tcp/ktp-live-canary.evidence.md"
    ));
    assert!(stdout.contains(
        "tunnel_evidence_file=logs/ktp_tcp/tunnel-echo.evidence.md tunnel_profile=rdp-like tunnel_clients=4 tunnel_rounds=8 tunnel_total_payload_bytes=39680 rtt_micros_p50=5958 rtt_micros_p95=26909 rtt_micros_p99=30755 rtt_micros_max=30755 rtt_client_p95_spread_micros=16954"
    ));
    assert!(stdout.contains("ktp_tcp_crypto=ktp_aead ktp_tcp_evidence=present"));
    assert!(stdout.contains(
        "ktp_tcp_tunnel_rtt_evidence=present profile=rdp-like clients=4 rounds=8 rtt_micros_p95=26909 rtt_client_p95_spread_micros=16954"
    ));
}

#[test]
fn ktp_local_backend_matrix_summary_require_pass_rejects_failed_rows() {
    let summary_path = write_temp_summary(
        "ktp-local-backend-matrix-summary-fail",
        r#"carrier	ktp_tcp	ktp_crypto	status	log_dir	summary_file	ktp_evidence_file
websocket	false	-	pass	logs/websocket	logs/websocket/agent.summary.md	-
ktp_tcp	true	ktp_aead	fail	logs/ktp_tcp	-	-
"#,
    );

    let output = Command::new(summary_exe())
        .arg("--require-pass")
        .arg(&summary_path)
        .output()
        .expect("ktp-local-backend-matrix-summary should run");

    assert_eq!(
        output.status.code(),
        Some(3),
        "require-pass should exit 3: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("carrier matrix row carrier=ktp_tcp status=fail failed require-pass gate")
    );
}

#[test]
fn ktp_local_backend_matrix_summary_require_ktp_aead_rejects_missing_crypto() {
    let summary_path = write_temp_summary(
        "ktp-local-backend-matrix-summary-missing-crypto",
        r#"carrier	ktp_tcp	ktp_crypto	status	log_dir	summary_file	ktp_evidence_file
websocket	false	-	pass	logs/websocket	logs/websocket/agent.summary.md	-
ktp_tcp	true	-	pass	logs/ktp_tcp	logs/ktp_tcp/agent.summary.md	logs/ktp_tcp/ktp-live-canary.evidence.md
"#,
    );

    let output = Command::new(summary_exe())
        .arg("--require-ktp-aead")
        .arg(&summary_path)
        .output()
        .expect("ktp-local-backend-matrix-summary should run");

    assert_eq!(
        output.status.code(),
        Some(3),
        "missing KTP AEAD should exit 3: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("carrier matrix missing pass row with carrier=ktp_tcp ktp_crypto=ktp_aead")
    );
}

#[test]
fn ktp_local_backend_matrix_summary_require_ktp_tunnel_rtt_rejects_missing_rtt_evidence() {
    let summary_path = write_temp_summary(
        "ktp-local-backend-matrix-summary-missing-rtt",
        r#"carrier	ktp_tcp	ktp_crypto	status	log_dir	summary_file	ktp_evidence_file	tunnel_evidence_file	tunnel_profile	tunnel_clients	tunnel_rounds	tunnel_total_payload_bytes	rtt_micros_p50	rtt_micros_p95	rtt_micros_p99	rtt_micros_max	rtt_client_p95_spread_micros
websocket	false	-	pass	logs/websocket	logs/websocket/agent.summary.md	-	-	-	-	-	-	-	-	-	-	-
ktp_tcp	true	ktp_aead	pass	logs/ktp_tcp	logs/ktp_tcp/agent.summary.md	logs/ktp_tcp/ktp-live-canary.evidence.md	-	-	-	-	-	-	-	-	-	-
"#,
    );

    let output = Command::new(summary_exe())
        .arg("--require-ktp-tunnel-rtt")
        .arg(&summary_path)
        .output()
        .expect("ktp-local-backend-matrix-summary should run");

    assert_eq!(
        output.status.code(),
        Some(3),
        "missing tunnel RTT evidence should exit 3: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("carrier matrix missing pass row with carrier=ktp_tcp tunnel RTT evidence")
    );
}

#[test]
fn ktp_local_backend_matrix_summary_require_ktp_rdp_like_rtt_accepts_multi_client_sample() {
    let summary_path = write_temp_summary(
        "ktp-local-backend-matrix-summary-rdp-like-rtt",
        r#"carrier	ktp_tcp	ktp_crypto	status	log_dir	summary_file	ktp_evidence_file	tunnel_evidence_file	tunnel_profile	tunnel_clients	tunnel_rounds	tunnel_total_payload_bytes	rtt_micros_p50	rtt_micros_p95	rtt_micros_p99	rtt_micros_max	rtt_client_p95_spread_micros
websocket	false	-	pass	logs/websocket	logs/websocket/agent.summary.md	-	logs/websocket/tunnel-echo.evidence.md	fixed	1	1	64	1000	1100	1200	1300	0
ktp_tcp	true	ktp_aead	pass	logs/ktp_tcp	logs/ktp_tcp/agent.summary.md	logs/ktp_tcp/ktp-live-canary.evidence.md	logs/ktp_tcp/tunnel-echo.evidence.md	rdp-like	4	8	39680	5958	26909	30755	30755	16954
"#,
    );

    let output = Command::new(summary_exe())
        .arg("--require-ktp-rdp-like-rtt")
        .arg(&summary_path)
        .output()
        .expect("ktp-local-backend-matrix-summary should run");

    assert!(
        output.status.success(),
        "rdp-like RTT gate should pass: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(
        "ktp_tcp_rdp_like_rtt_evidence=present clients=4 rounds=8 total_payload_bytes=39680 rtt_micros_p95=26909 rtt_client_p95_spread_micros=16954"
    ));
}

#[test]
fn ktp_local_backend_matrix_summary_require_ktp_rdp_like_rtt_rejects_trivial_sample() {
    let summary_path = write_temp_summary(
        "ktp-local-backend-matrix-summary-trivial-rtt",
        r#"carrier	ktp_tcp	ktp_crypto	status	log_dir	summary_file	ktp_evidence_file	tunnel_evidence_file	tunnel_profile	tunnel_clients	tunnel_rounds	tunnel_total_payload_bytes	rtt_micros_p50	rtt_micros_p95	rtt_micros_p99	rtt_micros_max	rtt_client_p95_spread_micros
websocket	false	-	pass	logs/websocket	logs/websocket/agent.summary.md	-	logs/websocket/tunnel-echo.evidence.md	fixed	1	1	64	1000	1100	1200	1300	0
ktp_tcp	true	ktp_aead	pass	logs/ktp_tcp	logs/ktp_tcp/agent.summary.md	logs/ktp_tcp/ktp-live-canary.evidence.md	logs/ktp_tcp/tunnel-echo.evidence.md	fixed	1	1	64	1000	1100	1200	1300	0
"#,
    );

    let output = Command::new(summary_exe())
        .arg("--require-ktp-rdp-like-rtt")
        .arg(&summary_path)
        .output()
        .expect("ktp-local-backend-matrix-summary should run");

    assert_eq!(
        output.status.code(),
        Some(3),
        "trivial tunnel RTT evidence should exit 3: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(
        "carrier matrix missing pass row with carrier=ktp_tcp rdp-like multi-client RTT evidence"
    ));
}

#[test]
fn ktp_local_backend_matrix_summary_rejects_rdp_like_rtt_p95_above_threshold() {
    let summary_path = write_temp_summary(
        "ktp-local-backend-matrix-summary-high-p95",
        r#"carrier	ktp_tcp	ktp_crypto	status	log_dir	summary_file	ktp_evidence_file	tunnel_evidence_file	tunnel_profile	tunnel_clients	tunnel_rounds	tunnel_total_payload_bytes	rtt_micros_p50	rtt_micros_p95	rtt_micros_p99	rtt_micros_max	rtt_client_p95_spread_micros
websocket	false	-	pass	logs/websocket	logs/websocket/agent.summary.md	-	logs/websocket/tunnel-echo.evidence.md	fixed	1	1	64	1000	1100	1200	1300	0
ktp_tcp	true	ktp_aead	pass	logs/ktp_tcp	logs/ktp_tcp/agent.summary.md	logs/ktp_tcp/ktp-live-canary.evidence.md	logs/ktp_tcp/tunnel-echo.evidence.md	rdp-like	4	8	39680	5958	600000	650000	700000	16954
"#,
    );

    let output = Command::new(summary_exe())
        .arg("--require-ktp-rdp-like-rtt")
        .arg("--max-ktp-rdp-like-rtt-p95-micros")
        .arg("250000")
        .arg(&summary_path)
        .output()
        .expect("ktp-local-backend-matrix-summary should run");

    assert_eq!(
        output.status.code(),
        Some(3),
        "high rdp-like RTT p95 should exit 3: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ktp_tcp rdp-like rtt_micros_p95 600000 exceeds max 250000"));
}

#[test]
fn ktp_local_backend_matrix_summary_rejects_rdp_like_client_spread_above_threshold() {
    let summary_path = write_temp_summary(
        "ktp-local-backend-matrix-summary-high-spread",
        r#"carrier	ktp_tcp	ktp_crypto	status	log_dir	summary_file	ktp_evidence_file	tunnel_evidence_file	tunnel_profile	tunnel_clients	tunnel_rounds	tunnel_total_payload_bytes	rtt_micros_p50	rtt_micros_p95	rtt_micros_p99	rtt_micros_max	rtt_client_p95_spread_micros
websocket	false	-	pass	logs/websocket	logs/websocket/agent.summary.md	-	logs/websocket/tunnel-echo.evidence.md	fixed	1	1	64	1000	1100	1200	1300	0
ktp_tcp	true	ktp_aead	pass	logs/ktp_tcp	logs/ktp_tcp/agent.summary.md	logs/ktp_tcp/ktp-live-canary.evidence.md	logs/ktp_tcp/tunnel-echo.evidence.md	rdp-like	4	8	39680	5958	26909	30755	30755	300000
"#,
    );

    let output = Command::new(summary_exe())
        .arg("--require-ktp-rdp-like-rtt")
        .arg("--max-ktp-rdp-like-client-p95-spread-micros")
        .arg("100000")
        .arg(&summary_path)
        .output()
        .expect("ktp-local-backend-matrix-summary should run");

    assert_eq!(
        output.status.code(),
        Some(3),
        "high rdp-like client spread should exit 3: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ktp_tcp rdp-like rtt_client_p95_spread_micros 300000 exceeds max 100000")
    );
}

#[test]
fn ktp_local_backend_matrix_summary_rejects_missing_required_columns() {
    let summary_path = write_temp_summary(
        "ktp-local-backend-matrix-summary-missing-column",
        r#"carrier	status
websocket	pass
"#,
    );

    let output = Command::new(summary_exe())
        .arg(&summary_path)
        .output()
        .expect("ktp-local-backend-matrix-summary should run");

    assert_eq!(
        output.status.code(),
        Some(2),
        "missing-column summary should exit 2: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("missing required column: ktp_crypto"));
}

fn write_temp_summary(prefix: &str, content: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}.tsv", std::process::id()));
    std::fs::write(&path, content).expect("summary fixture should be written");
    path
}

fn summary_exe() -> String {
    std::env::var("CARGO_BIN_EXE_ktp-local-backend-matrix-summary")
        .expect("ktp-local-backend-matrix-summary binary should be built by cargo")
}
