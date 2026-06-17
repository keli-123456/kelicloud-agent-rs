use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn ktp_policy_summary_cli_reports_fixed_better_when_adaptive_regresses() {
    let csv_path = write_temp_csv(
        "ktp-policy-summary-fixed",
        r#"profile,runs,clients,frames,payload_bytes,relay_batch_frames,relay_batch_policy,relay_batch_frames_effective,elapsed_ms_min,elapsed_ms_median,elapsed_ms_max,throughput_mib_s_min,throughput_mib_s_median,throughput_mib_s_max,rtt_micros_p50,rtt_micros_p95,rtt_micros_p99,rtt_micros_max,rtt_client_p95_micros_min,rtt_client_p95_micros_max,rtt_client_p95_spread_micros,rtt_client_max_micros_max,relay_turns,relay_wait_turns,ingress_batches,egress_batches,ingress_max_batch_frames,egress_max_batch_frames
rdp-like,2,4,64,8192,64,fixed,64,19.807,27.361,34.914,6.755,9.331,11.907,227,949,2940,5768,858,1106,248,5768,784,609,392,392,4,4
rdp-like,2,4,64,8192,64,adaptive,32,34.063,44.550,55.037,4.285,5.604,6.924,228,1456,3654,8002,703,2761,2058,8002,856,754,425,397,4,4
"#,
    );

    let output = Command::new(policy_summary_exe())
        .arg(&csv_path)
        .output()
        .expect("ktp-policy-summary should run");

    assert!(
        output.status.success(),
        "ktp-policy-summary failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ktp_policy_summary rows=2 pairs=1"));
    assert!(stdout.contains("clients=4 relay_batch_frames=64"));
    assert!(stdout.contains("fixed_effective=64 adaptive_effective=32"));
    assert!(stdout.contains("throughput_delta_pct=-39.94"));
    assert!(stdout.contains("rtt_p95_delta_pct=53.42"));
    assert!(stdout.contains("client_p95_spread_delta_pct=729.84"));
    assert!(stdout.contains("verdict=fixed_better"));
}

#[test]
fn ktp_policy_summary_cli_fail_gate_rejects_fixed_better_verdict() {
    let csv_path = write_temp_csv("ktp-policy-summary-fail-gate", fixed_better_csv());

    let output = Command::new(policy_summary_exe())
        .arg("--fail-on-fixed-better")
        .arg(&csv_path)
        .output()
        .expect("ktp-policy-summary should run");

    assert!(
        !output.status.success(),
        "ktp-policy-summary unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("verdict=fixed_better"));
    assert!(stderr.contains("fixed_better verdict failed KTP policy gate"));
}

#[test]
fn ktp_policy_summary_cli_fail_gate_allows_adaptive_better_verdict() {
    let csv_path = write_temp_csv("ktp-policy-summary-pass-gate", adaptive_better_csv());

    let output = Command::new(policy_summary_exe())
        .arg("--fail-on-fixed-better")
        .arg(&csv_path)
        .output()
        .expect("ktp-policy-summary should run");

    assert!(
        output.status.success(),
        "ktp-policy-summary failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("verdict=adaptive_better"));
}

#[test]
fn ktp_policy_summary_cli_fail_gate_allows_same_effective_policy() {
    let csv_path = write_temp_csv("ktp-policy-summary-same-effective", same_effective_csv());

    let output = Command::new(policy_summary_exe())
        .arg("--fail-on-fixed-better")
        .arg(&csv_path)
        .output()
        .expect("ktp-policy-summary should run");

    assert!(
        output.status.success(),
        "ktp-policy-summary failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("fixed_effective=64 adaptive_effective=64"));
    assert!(stdout.contains("verdict=same_effective"));
}

#[test]
fn ktp_policy_summary_cli_reports_adaptive_better_when_all_primary_metrics_improve() {
    let csv_path = write_temp_csv("ktp-policy-summary-adaptive", adaptive_better_csv());

    let output = Command::new(policy_summary_exe())
        .arg(&csv_path)
        .output()
        .expect("ktp-policy-summary should run");

    assert!(
        output.status.success(),
        "ktp-policy-summary failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("clients=8 relay_batch_frames=64"));
    assert!(stdout.contains("fixed_effective=64 adaptive_effective=16"));
    assert!(stdout.contains("throughput_delta_pct=10.00"));
    assert!(stdout.contains("rtt_p95_delta_pct=-20.00"));
    assert!(stdout.contains("client_p95_spread_delta_pct=-50.00"));
    assert!(stdout.contains("verdict=adaptive_better"));
}

#[test]
fn ktp_policy_summary_cli_rejects_csv_without_policy_pairs() {
    let csv_path = write_temp_csv(
        "ktp-policy-summary-missing-pair",
        r#"profile,runs,clients,frames,payload_bytes,relay_batch_frames,relay_batch_policy,relay_batch_frames_effective,elapsed_ms_min,elapsed_ms_median,elapsed_ms_max,throughput_mib_s_min,throughput_mib_s_median,throughput_mib_s_max,rtt_micros_p50,rtt_micros_p95,rtt_micros_p99,rtt_micros_max,rtt_client_p95_micros_min,rtt_client_p95_micros_max,rtt_client_p95_spread_micros,rtt_client_max_micros_max,relay_turns,relay_wait_turns,ingress_batches,egress_batches,ingress_max_batch_frames,egress_max_batch_frames
rdp-like,2,4,64,8192,64,fixed,64,19.807,27.361,34.914,6.755,9.331,11.907,227,949,2940,5768,858,1106,248,5768,784,609,392,392,4,4
"#,
    );

    let output = Command::new(policy_summary_exe())
        .arg(&csv_path)
        .output()
        .expect("ktp-policy-summary should run");

    assert!(
        !output.status.success(),
        "ktp-policy-summary unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no fixed/adaptive policy pairs found"));
}

fn write_temp_csv(prefix: &str, content: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}.csv", std::process::id()));
    std::fs::write(&path, content).expect("CSV fixture should be written");
    path
}

fn policy_summary_exe() -> String {
    std::env::var("CARGO_BIN_EXE_ktp-policy-summary")
        .expect("ktp-policy-summary binary should be built by cargo")
}

fn fixed_better_csv() -> &'static str {
    r#"profile,runs,clients,frames,payload_bytes,relay_batch_frames,relay_batch_policy,relay_batch_frames_effective,elapsed_ms_min,elapsed_ms_median,elapsed_ms_max,throughput_mib_s_min,throughput_mib_s_median,throughput_mib_s_max,rtt_micros_p50,rtt_micros_p95,rtt_micros_p99,rtt_micros_max,rtt_client_p95_micros_min,rtt_client_p95_micros_max,rtt_client_p95_spread_micros,rtt_client_max_micros_max,relay_turns,relay_wait_turns,ingress_batches,egress_batches,ingress_max_batch_frames,egress_max_batch_frames
rdp-like,2,4,64,8192,64,fixed,64,19.807,27.361,34.914,6.755,9.331,11.907,227,949,2940,5768,858,1106,248,5768,784,609,392,392,4,4
rdp-like,2,4,64,8192,64,adaptive,32,34.063,44.550,55.037,4.285,5.604,6.924,228,1456,3654,8002,703,2761,2058,8002,856,754,425,397,4,4
"#
}

fn adaptive_better_csv() -> &'static str {
    r#"profile,runs,clients,frames,payload_bytes,relay_batch_frames,relay_batch_policy,relay_batch_frames_effective,elapsed_ms_min,elapsed_ms_median,elapsed_ms_max,throughput_mib_s_min,throughput_mib_s_median,throughput_mib_s_max,rtt_micros_p50,rtt_micros_p95,rtt_micros_p99,rtt_micros_max,rtt_client_p95_micros_min,rtt_client_p95_micros_max,rtt_client_p95_spread_micros,rtt_client_max_micros_max,relay_turns,relay_wait_turns,ingress_batches,egress_batches,ingress_max_batch_frames,egress_max_batch_frames
rdp-like,5,8,64,8192,64,fixed,64,20.000,25.000,30.000,8.000,10.000,12.000,200,1000,2000,4000,800,1200,400,4000,100,80,60,60,8,8
rdp-like,5,8,64,8192,64,adaptive,16,18.000,23.000,28.000,9.000,11.000,13.000,180,800,1700,3000,700,1000,200,3000,120,90,70,70,6,6
"#
}

fn same_effective_csv() -> &'static str {
    r#"profile,runs,clients,frames,payload_bytes,relay_batch_frames,relay_batch_policy,relay_batch_frames_effective,elapsed_ms_min,elapsed_ms_median,elapsed_ms_max,throughput_mib_s_min,throughput_mib_s_median,throughput_mib_s_max,rtt_micros_p50,rtt_micros_p95,rtt_micros_p99,rtt_micros_max,rtt_client_p95_micros_min,rtt_client_p95_micros_max,rtt_client_p95_spread_micros,rtt_client_max_micros_max,relay_turns,relay_wait_turns,ingress_batches,egress_batches,ingress_max_batch_frames,egress_max_batch_frames
rdp-like,5,4,64,8192,64,fixed,64,16.731,24.788,28.730,8.209,9.514,14.096,216,891,2156,3575,841,922,81,3575,1848,1434,950,957,7,4
rdp-like,5,4,64,8192,64,adaptive,64,20.894,27.983,38.629,6.105,8.428,11.287,264,954,2520,4504,839,1087,248,4504,1812,1377,951,956,6,5
"#
}
