use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn ktp_tunnel_matrix_summary_reports_pass_rows_and_extremes() {
    let summary_path = write_temp_summary(
        "ktp-tunnel-matrix-summary-pass",
        r#"relay_batch_policy	clients	relay_adaptive_high_sessions	relay_adaptive_elevated_dwell_us	relay_adaptive_severe_dwell_us	relay_adaptive_elevated_cap	relay_adaptive_severe_cap	rounds	profile	payload_bytes	status	elapsed_millis	log_dir	tunnel_evidence_file	ktp_evidence_file	total_payload_bytes	echo_elapsed_micros	rtt_micros_p50	rtt_micros_p95	rtt_micros_p99	rtt_micros_max	rtt_client_p95_spread_micros	socket_read_batches	socket_read_frames	socket_read_max_batch_frames	socket_write_batches	socket_write_frames	socket_write_max_batch_frames	socket_write_batch_limit_max	socket_write_batch_limit_min	socket_write_batch_limit_last
fixed	1	8	50000	250000	16	8	8	rdp-like	8192	pass	123	logs/fixed/clients-1	logs/fixed/clients-1/tunnel-echo.evidence.md	logs/fixed/clients-1/ktp-live-canary.evidence.md	9920	100000	100	200	300	400	0	3	40	2	2	40	5	64	64	64
adaptive	4	4	40000	120000	24	6	8	rdp-like	8192	pass	456	logs/adaptive/clients-4	logs/adaptive/clients-4/tunnel-echo.evidence.md	logs/adaptive/clients-4/ktp-live-canary.evidence.md	39680	200000	500	600	700	800	90	12	224	11	10	236	12	64	16	16
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
    assert!(stdout.contains("policy=fixed clients=1 status=pass elapsed_millis=123 total_payload_bytes=9920 throughput_mib_s=0.077 echo_elapsed_micros=100000 echo_throughput_mib_s=0.095 rtt_micros_p95=200 rtt_client_p95_spread_micros=0 socket_read_max_batch_frames=2 socket_write_max_batch_frames=5 socket_write_batch_limit_max=64 socket_write_batch_limit_min=64 socket_write_batch_limit_last=64"));
    assert!(stdout.contains("policy=adaptive clients=4 status=pass elapsed_millis=456 total_payload_bytes=39680 throughput_mib_s=0.083 echo_elapsed_micros=200000 echo_throughput_mib_s=0.189 rtt_micros_p95=600 rtt_client_p95_spread_micros=90 socket_read_max_batch_frames=11 socket_write_max_batch_frames=12 socket_write_batch_limit_max=64 socket_write_batch_limit_min=16 socket_write_batch_limit_last=16"));
    assert!(stdout.contains("relay_adaptive_high_sessions=8 relay_adaptive_elevated_dwell_us=50000 relay_adaptive_severe_dwell_us=250000 relay_adaptive_elevated_cap=16 relay_adaptive_severe_cap=8"));
    assert!(stdout.contains("relay_adaptive_high_sessions=4 relay_adaptive_elevated_dwell_us=40000 relay_adaptive_severe_dwell_us=120000 relay_adaptive_elevated_cap=24 relay_adaptive_severe_cap=6"));
    assert!(stdout.contains("max_rtt_micros_p95=600 policy=adaptive clients=4"));
    assert!(stdout.contains("max_rtt_client_p95_spread_micros=90 policy=adaptive clients=4"));
    assert!(stdout.contains("max_socket_read_max_batch_frames=11 policy=adaptive clients=4"));
    assert!(stdout.contains("max_socket_write_max_batch_frames=12 policy=adaptive clients=4"));
    assert!(stdout.contains("max_socket_write_batch_limit_max=64 policy=fixed clients=1"));
    assert!(stdout.contains("min_socket_write_batch_limit_min=16 policy=adaptive clients=4"));
    assert!(stdout.contains("min_throughput_mib_s=0.077 policy=fixed clients=1"));
    assert!(stdout.contains("min_echo_throughput_mib_s=0.095 policy=fixed clients=1"));
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
fn ktp_tunnel_matrix_summary_recommends_policy_for_each_pair() {
    let summary_path = write_temp_summary(
        "ktp-tunnel-matrix-summary-policy-recommend",
        r#"relay_batch_policy	clients	rounds	profile	payload_bytes	status	elapsed_millis	log_dir	tunnel_evidence_file	ktp_evidence_file	total_payload_bytes	rtt_micros_p50	rtt_micros_p95	rtt_micros_p99	rtt_micros_max	rtt_client_p95_spread_micros	socket_read_batches	socket_read_frames	socket_read_max_batch_frames
fixed	2	8	rdp-like	8192	pass	500	logs/fixed/clients-2	logs/fixed/clients-2/tunnel-echo.evidence.md	logs/fixed/clients-2/ktp-live-canary.evidence.md	19840	700	1000	1200	1500	200	12	112	6
adaptive	2	8	rdp-like	8192	pass	450	logs/adaptive/clients-2	logs/adaptive/clients-2/tunnel-echo.evidence.md	logs/adaptive/clients-2/ktp-live-canary.evidence.md	19840	500	800	900	1000	100	14	112	8
fixed	4	8	rdp-like	8192	pass	500	logs/fixed/clients-4	logs/fixed/clients-4/tunnel-echo.evidence.md	logs/fixed/clients-4/ktp-live-canary.evidence.md	39680	400	800	900	1000	300	12	224	8
adaptive	4	8	rdp-like	8192	pass	450	logs/adaptive/clients-4	logs/adaptive/clients-4/tunnel-echo.evidence.md	logs/adaptive/clients-4/ktp-live-canary.evidence.md	39680	450	900	1000	1200	100	14	224	11
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
    assert!(stdout.contains(
        "policy_recommend clients=2 recommended=adaptive verdict=adaptive_better reason=adaptive_not_worse"
    ));
    assert!(stdout.contains(
        "policy_recommend clients=4 recommended=manual_review verdict=mixed reason=metric_tradeoff"
    ));
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
fn ktp_tunnel_matrix_summary_threshold_gates_reject_latency_and_spread_regressions() {
    let summary_path = write_temp_summary(
        "ktp-tunnel-matrix-summary-threshold-gates",
        r#"relay_batch_policy	clients	rounds	profile	payload_bytes	status	elapsed_millis	log_dir	tunnel_evidence_file	ktp_evidence_file	total_payload_bytes	rtt_micros_p50	rtt_micros_p95	rtt_micros_p99	rtt_micros_max	rtt_client_p95_spread_micros	socket_read_batches	socket_read_frames	socket_read_max_batch_frames
fixed	1	8	rdp-like	8192	pass	123	logs/fixed/clients-1	logs/fixed/clients-1/tunnel-echo.evidence.md	logs/fixed/clients-1/ktp-live-canary.evidence.md	9920	100	700	800	900	0	3	40	2
adaptive	4	8	rdp-like	8192	pass	456	logs/adaptive/clients-4	logs/adaptive/clients-4/tunnel-echo.evidence.md	logs/adaptive/clients-4/ktp-live-canary.evidence.md	39680	500	600	700	800	90	12	224	11
"#,
    );

    let output = Command::new(summary_exe())
        .arg("--max-rtt-p95-micros")
        .arg("650")
        .arg("--max-client-p95-spread-micros")
        .arg("80")
        .arg(&summary_path)
        .output()
        .expect("ktp-tunnel-matrix-summary should run");

    assert_eq!(
        output.status.code(),
        Some(3),
        "threshold gates should exit 3: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("ktp_tunnel_matrix_summary rows=2 pass=2 fail=0 timeout=0 status=pass"));
    assert!(stderr
        .contains("tunnel matrix row policy=fixed clients=1 rtt_micros_p95=700 exceeds max 650"));
    assert!(stderr.contains(
        "tunnel matrix row policy=adaptive clients=4 rtt_client_p95_spread_micros=90 exceeds max 80"
    ));
}

#[test]
fn ktp_tunnel_matrix_summary_threshold_gate_rejects_low_throughput() {
    let summary_path = write_temp_summary(
        "ktp-tunnel-matrix-summary-throughput-gate",
        r#"relay_batch_policy	clients	rounds	profile	payload_bytes	status	elapsed_millis	log_dir	tunnel_evidence_file	ktp_evidence_file	total_payload_bytes	rtt_micros_p50	rtt_micros_p95	rtt_micros_p99	rtt_micros_max	rtt_client_p95_spread_micros	socket_read_batches	socket_read_frames	socket_read_max_batch_frames
fixed	1	8	rdp-like	8192	pass	2000	logs/fixed/clients-1	logs/fixed/clients-1/tunnel-echo.evidence.md	logs/fixed/clients-1/ktp-live-canary.evidence.md	1048576	100	200	300	400	0	3	40	2
adaptive	4	8	rdp-like	8192	pass	1000	logs/adaptive/clients-4	logs/adaptive/clients-4/tunnel-echo.evidence.md	logs/adaptive/clients-4/ktp-live-canary.evidence.md	4194304	500	600	700	800	90	12	224	11
"#,
    );

    let output = Command::new(summary_exe())
        .arg("--min-throughput-mib-s")
        .arg("1")
        .arg(&summary_path)
        .output()
        .expect("ktp-tunnel-matrix-summary should run");

    assert_eq!(
        output.status.code(),
        Some(3),
        "throughput gate should exit 3: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("min_throughput_mib_s=0.500 policy=fixed clients=1"));
    assert!(stderr.contains(
        "tunnel matrix row policy=fixed clients=1 throughput_mib_s=0.500 below min 1.000"
    ));
}

#[test]
fn ktp_tunnel_matrix_summary_threshold_gate_rejects_low_echo_throughput() {
    let summary_path = write_temp_summary(
        "ktp-tunnel-matrix-summary-echo-throughput-gate",
        r#"relay_batch_policy	clients	rounds	profile	payload_bytes	status	elapsed_millis	log_dir	tunnel_evidence_file	ktp_evidence_file	total_payload_bytes	echo_elapsed_micros	rtt_micros_p50	rtt_micros_p95	rtt_micros_p99	rtt_micros_max	rtt_client_p95_spread_micros	socket_read_batches	socket_read_frames	socket_read_max_batch_frames
fixed	1	8	rdp-like	8192	pass	60000	logs/fixed/clients-1	logs/fixed/clients-1/tunnel-echo.evidence.md	logs/fixed/clients-1/ktp-live-canary.evidence.md	1048576	2000000	100	200	300	400	0	3	40	2
adaptive	4	8	rdp-like	8192	pass	60000	logs/adaptive/clients-4	logs/adaptive/clients-4/tunnel-echo.evidence.md	logs/adaptive/clients-4/ktp-live-canary.evidence.md	4194304	1000000	500	600	700	800	90	12	224	11
"#,
    );

    let output = Command::new(summary_exe())
        .arg("--min-echo-throughput-mib-s")
        .arg("1")
        .arg(&summary_path)
        .output()
        .expect("ktp-tunnel-matrix-summary should run");

    assert_eq!(
        output.status.code(),
        Some(3),
        "echo throughput gate should exit 3: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("min_echo_throughput_mib_s=0.500 policy=fixed clients=1"));
    assert!(stderr.contains(
        "tunnel matrix row policy=fixed clients=1 echo_throughput_mib_s=0.500 below min 1.000"
    ));
}

#[test]
fn ktp_tunnel_matrix_summary_expect_matrix_rejects_missing_policy_client_rows() {
    let summary_path = write_temp_summary(
        "ktp-tunnel-matrix-summary-missing-expected-row",
        r#"relay_batch_policy	clients	rounds	profile	payload_bytes	status	elapsed_millis	log_dir	tunnel_evidence_file	ktp_evidence_file	total_payload_bytes	rtt_micros_p50	rtt_micros_p95	rtt_micros_p99	rtt_micros_max	rtt_client_p95_spread_micros	socket_read_batches	socket_read_frames	socket_read_max_batch_frames
fixed	1	8	rdp-like	8192	pass	500	logs/fixed/clients-1	logs/fixed/clients-1/tunnel-echo.evidence.md	logs/fixed/clients-1/ktp-live-canary.evidence.md	9920	700	1000	1200	1500	0	12	40	2
adaptive	1	8	rdp-like	8192	pass	450	logs/adaptive/clients-1	logs/adaptive/clients-1/tunnel-echo.evidence.md	logs/adaptive/clients-1/ktp-live-canary.evidence.md	9920	500	800	900	1000	0	14	40	3
fixed	2	8	rdp-like	8192	pass	500	logs/fixed/clients-2	logs/fixed/clients-2/tunnel-echo.evidence.md	logs/fixed/clients-2/ktp-live-canary.evidence.md	19840	700	1000	1200	1500	200	12	80	4
"#,
    );

    let output = Command::new(summary_exe())
        .arg("--expect-policies")
        .arg("fixed adaptive")
        .arg("--expect-clients")
        .arg("1 2")
        .arg(&summary_path)
        .output()
        .expect("ktp-tunnel-matrix-summary should run");

    assert_eq!(
        output.status.code(),
        Some(3),
        "missing expected row should exit 3: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("ktp_tunnel_matrix_summary rows=3 pass=3 fail=0 timeout=0 status=pass"));
    assert!(stderr.contains("missing tunnel matrix row policy=adaptive clients=2"));
}

#[test]
fn ktp_tunnel_matrix_summary_expect_matrix_accepts_all_policy_client_rows() {
    let summary_path = write_temp_summary(
        "ktp-tunnel-matrix-summary-all-expected-rows",
        r#"relay_batch_policy	clients	rounds	profile	payload_bytes	status	elapsed_millis	log_dir	tunnel_evidence_file	ktp_evidence_file	total_payload_bytes	rtt_micros_p50	rtt_micros_p95	rtt_micros_p99	rtt_micros_max	rtt_client_p95_spread_micros	socket_read_batches	socket_read_frames	socket_read_max_batch_frames
fixed	1	8	rdp-like	8192	pass	500	logs/fixed/clients-1	logs/fixed/clients-1/tunnel-echo.evidence.md	logs/fixed/clients-1/ktp-live-canary.evidence.md	9920	700	1000	1200	1500	0	12	40	2
adaptive	1	8	rdp-like	8192	pass	450	logs/adaptive/clients-1	logs/adaptive/clients-1/tunnel-echo.evidence.md	logs/adaptive/clients-1/ktp-live-canary.evidence.md	9920	500	800	900	1000	0	14	40	3
fixed	2	8	rdp-like	8192	pass	500	logs/fixed/clients-2	logs/fixed/clients-2/tunnel-echo.evidence.md	logs/fixed/clients-2/ktp-live-canary.evidence.md	19840	700	1000	1200	1500	200	12	80	4
adaptive	2	8	rdp-like	8192	pass	450	logs/adaptive/clients-2	logs/adaptive/clients-2/tunnel-echo.evidence.md	logs/adaptive/clients-2/ktp-live-canary.evidence.md	19840	500	800	900	1000	100	14	80	5
"#,
    );

    let output = Command::new(summary_exe())
        .arg("--expect-policies")
        .arg("fixed,adaptive")
        .arg("--expect-clients")
        .arg("1,2")
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
    assert!(stdout.contains("expected_matrix policies=fixed,adaptive clients=1,2 status=pass"));
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
