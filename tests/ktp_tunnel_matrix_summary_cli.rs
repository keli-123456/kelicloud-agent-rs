use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn ktp_tunnel_matrix_summary_reports_pass_rows_and_extremes() {
    let summary_path = write_temp_summary(
        "ktp-tunnel-matrix-summary-pass",
        r#"relay_batch_policy	clients	rounds	profile	payload_bytes	status	elapsed_millis	log_dir	tunnel_evidence_file	ktp_evidence_file	total_payload_bytes	rtt_micros_p50	rtt_micros_p95	rtt_micros_p99	rtt_micros_max	rtt_client_p95_spread_micros	socket_read_batches	socket_read_frames	socket_read_max_batch_frames	socket_write_batches	socket_write_frames	socket_write_max_batch_frames	socket_write_batch_limit_max
fixed	1	8	rdp-like	8192	pass	123	logs/fixed/clients-1	logs/fixed/clients-1/tunnel-echo.evidence.md	logs/fixed/clients-1/ktp-live-canary.evidence.md	9920	100	200	300	400	0	3	40	2	2	40	5	64
adaptive	4	8	rdp-like	8192	pass	456	logs/adaptive/clients-4	logs/adaptive/clients-4/tunnel-echo.evidence.md	logs/adaptive/clients-4/ktp-live-canary.evidence.md	39680	500	600	700	800	90	12	224	11	10	236	12	16
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
    assert!(stdout.contains("policy=fixed clients=1 status=pass elapsed_millis=123 rtt_micros_p95=200 rtt_client_p95_spread_micros=0 socket_read_max_batch_frames=2 socket_write_max_batch_frames=5 socket_write_batch_limit_max=64"));
    assert!(stdout.contains("policy=adaptive clients=4 status=pass elapsed_millis=456 rtt_micros_p95=600 rtt_client_p95_spread_micros=90 socket_read_max_batch_frames=11 socket_write_max_batch_frames=12 socket_write_batch_limit_max=16"));
    assert!(stdout.contains("max_rtt_micros_p95=600 policy=adaptive clients=4"));
    assert!(stdout.contains("max_rtt_client_p95_spread_micros=90 policy=adaptive clients=4"));
    assert!(stdout.contains("max_socket_read_max_batch_frames=11 policy=adaptive clients=4"));
    assert!(stdout.contains("max_socket_write_max_batch_frames=12 policy=adaptive clients=4"));
    assert!(stdout.contains("max_socket_write_batch_limit_max=64 policy=fixed clients=1"));
}

#[test]
fn ktp_tunnel_matrix_summary_require_pass_rejects_failed_rows() {
    let summary_path = write_temp_summary(
        "ktp-tunnel-matrix-summary-fail",
        r#"relay_batch_policy	clients	rounds	profile	payload_bytes	status	elapsed_millis	log_dir	tunnel_evidence_file	ktp_evidence_file	total_payload_bytes	rtt_micros_p50	rtt_micros_p95	rtt_micros_p99	rtt_micros_max	rtt_client_p95_spread_micros	socket_read_batches	socket_read_frames	socket_read_max_batch_frames
fixed	1	8	rdp-like	8192	pass	123	logs/fixed/clients-1	logs/fixed/clients-1/tunnel-echo.evidence.md	logs/fixed/clients-1/ktp-live-canary.evidence.md	9920	100	200	300	400	0	3	40	2
adaptive	4	8	rdp-like	8192	fail	456	logs/adaptive/clients-4	-	-	-	-	-	-	-	-	-	-	-
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
    assert!(stderr.contains(
        "tunnel matrix row policy=adaptive clients=4 status=fail failed require-pass gate"
    ));
}

#[test]
fn ktp_tunnel_matrix_summary_reports_fixed_adaptive_pair_verdicts() {
    let summary_path = write_temp_summary(
        "ktp-tunnel-matrix-summary-policy-compare",
        r#"relay_batch_policy	clients	rounds	profile	payload_bytes	status	elapsed_millis	log_dir	tunnel_evidence_file	ktp_evidence_file	total_payload_bytes	rtt_micros_p50	rtt_micros_p95	rtt_micros_p99	rtt_micros_max	rtt_client_p95_spread_micros	socket_read_batches	socket_read_frames	socket_read_max_batch_frames
fixed	4	8	rdp-like	8192	pass	500	logs/fixed/clients-4	logs/fixed/clients-4/tunnel-echo.evidence.md	logs/fixed/clients-4/ktp-live-canary.evidence.md	39680	700	1000	1200	1500	200	12	224	8
adaptive	4	8	rdp-like	8192	pass	450	logs/adaptive/clients-4	logs/adaptive/clients-4/tunnel-echo.evidence.md	logs/adaptive/clients-4/ktp-live-canary.evidence.md	39680	500	800	900	1000	100	14	224	11
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
    assert!(stdout.contains("policy_compare clients=4 fixed_elapsed_millis=500 adaptive_elapsed_millis=450 elapsed_delta_pct=-10.00 fixed_rtt_micros_p95=1000 adaptive_rtt_micros_p95=800 rtt_p95_delta_pct=-20.00 fixed_rtt_client_p95_spread_micros=200 adaptive_rtt_client_p95_spread_micros=100 spread_delta_pct=-50.00 verdict=adaptive_better"));
}

#[test]
fn ktp_tunnel_matrix_summary_fail_gate_rejects_fixed_better_verdict() {
    let summary_path = write_temp_summary(
        "ktp-tunnel-matrix-summary-fixed-better",
        r#"relay_batch_policy	clients	rounds	profile	payload_bytes	status	elapsed_millis	log_dir	tunnel_evidence_file	ktp_evidence_file	total_payload_bytes	rtt_micros_p50	rtt_micros_p95	rtt_micros_p99	rtt_micros_max	rtt_client_p95_spread_micros	socket_read_batches	socket_read_frames	socket_read_max_batch_frames
fixed	4	8	rdp-like	8192	pass	400	logs/fixed/clients-4	logs/fixed/clients-4/tunnel-echo.evidence.md	logs/fixed/clients-4/ktp-live-canary.evidence.md	39680	400	700	900	1000	80	12	224	8
adaptive	4	8	rdp-like	8192	pass	500	logs/adaptive/clients-4	logs/adaptive/clients-4/tunnel-echo.evidence.md	logs/adaptive/clients-4/ktp-live-canary.evidence.md	39680	500	900	1000	1200	120	14	224	11
"#,
    );

    let output = Command::new(summary_exe())
        .arg("--fail-on-fixed-better")
        .arg(&summary_path)
        .output()
        .expect("ktp-tunnel-matrix-summary should run");

    assert_eq!(
        output.status.code(),
        Some(3),
        "fixed-better gate should exit 3: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("policy_compare clients=4"));
    assert!(stdout.contains("verdict=fixed_better"));
    assert!(stderr.contains(
        "fixed_better tunnel matrix verdict failed KTP tunnel policy gate for clients=4"
    ));
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
