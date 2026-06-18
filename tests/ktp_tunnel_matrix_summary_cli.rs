use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn ktp_tunnel_matrix_summary_reports_pass_rows_and_extremes() {
    let summary_path = write_temp_summary(
        "ktp-tunnel-matrix-summary-pass",
        r#"clients	rounds	profile	payload_bytes	status	elapsed_millis	log_dir	tunnel_evidence_file	ktp_evidence_file	total_payload_bytes	rtt_micros_p50	rtt_micros_p95	rtt_micros_p99	rtt_micros_max	rtt_client_p95_spread_micros	socket_read_batches	socket_read_frames	socket_read_max_batch_frames
1	8	rdp-like	8192	pass	123	logs/clients-1	logs/clients-1/tunnel-echo.evidence.md	logs/clients-1/ktp-live-canary.evidence.md	9920	100	200	300	400	0	3	40	2
4	8	rdp-like	8192	pass	456	logs/clients-4	logs/clients-4/tunnel-echo.evidence.md	logs/clients-4/ktp-live-canary.evidence.md	39680	500	600	700	800	90	12	224	11
"#,
    );

    let output = Command::new(summary_exe())
        .arg(&summary_path)
        .output()
        .expect("ktp-tunnel-matrix-summary should run");

    assert!(
        output.status.success(),
        "summary failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ktp_tunnel_matrix_summary rows=2 pass=2 fail=0 timeout=0 status=pass"));
    assert!(stdout.contains("clients=1 status=pass elapsed_millis=123 rtt_micros_p95=200 rtt_client_p95_spread_micros=0 socket_read_max_batch_frames=2"));
    assert!(stdout.contains("clients=4 status=pass elapsed_millis=456 rtt_micros_p95=600 rtt_client_p95_spread_micros=90 socket_read_max_batch_frames=11"));
    assert!(stdout.contains("max_rtt_micros_p95=600 clients=4"));
    assert!(stdout.contains("max_rtt_client_p95_spread_micros=90 clients=4"));
    assert!(stdout.contains("max_socket_read_max_batch_frames=11 clients=4"));
}

#[test]
fn ktp_tunnel_matrix_summary_require_pass_rejects_failed_rows() {
    let summary_path = write_temp_summary(
        "ktp-tunnel-matrix-summary-fail",
        r#"clients	rounds	profile	payload_bytes	status	elapsed_millis	log_dir	tunnel_evidence_file	ktp_evidence_file	total_payload_bytes	rtt_micros_p50	rtt_micros_p95	rtt_micros_p99	rtt_micros_max	rtt_client_p95_spread_micros	socket_read_batches	socket_read_frames	socket_read_max_batch_frames
1	8	rdp-like	8192	pass	123	logs/clients-1	logs/clients-1/tunnel-echo.evidence.md	logs/clients-1/ktp-live-canary.evidence.md	9920	100	200	300	400	0	3	40	2
4	8	rdp-like	8192	fail	456	logs/clients-4	-	-	-	-	-	-	-	-	-	-	-
"#,
    );

    let output = Command::new(summary_exe())
        .arg("--require-pass")
        .arg(&summary_path)
        .output()
        .expect("ktp-tunnel-matrix-summary should run");

    assert_eq!(
        output.status.code(),
        Some(3),
        "require-pass should exit 3: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("ktp_tunnel_matrix_summary rows=2 pass=1 fail=1 timeout=0 status=fail"));
    assert!(stderr.contains("tunnel matrix row clients=4 status=fail failed require-pass gate"));
}

#[test]
fn ktp_tunnel_matrix_summary_rejects_missing_required_columns() {
    let summary_path = write_temp_summary(
        "ktp-tunnel-matrix-summary-missing-column",
        r#"clients	status	elapsed_millis
1	pass	123
"#,
    );

    let output = Command::new(summary_exe())
        .arg(&summary_path)
        .output()
        .expect("ktp-tunnel-matrix-summary should run");

    assert_eq!(
        output.status.code(),
        Some(2),
        "missing-column summary should exit 2: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("missing required column: rtt_micros_p95"));
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
    std::env::var("CARGO_BIN_EXE_ktp-tunnel-matrix-summary")
        .expect("ktp-tunnel-matrix-summary binary should be built by cargo")
}
