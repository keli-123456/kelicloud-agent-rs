use std::process::Command;

#[test]
fn ktp_e2e_bench_cli_reports_runtime_ingress_egress_throughput() {
    let exe = std::env::var("CARGO_BIN_EXE_ktp-e2e-bench")
        .expect("ktp-e2e-bench binary should be built by cargo");

    let output = Command::new(exe)
        .args(["--frames", "3", "--payload-bytes", "128"])
        .output()
        .expect("ktp-e2e-bench should run");

    assert!(
        output.status.success(),
        "ktp-e2e-bench failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ktp_e2e_bench"));
    assert!(stdout.contains("mode=runtime_ingress_egress"));
    assert!(stdout.contains("transport=ktp_tcp"));
    assert!(stdout.contains("bridge=batch"));
    assert!(stdout.contains("clients=1"));
    assert!(stdout.contains("runs=1"));
    assert!(stdout.contains("frames=3"));
    assert!(stdout.contains("payload_bytes=128"));
    assert!(stdout.contains("bytes=384"));
    assert!(stdout.contains("elapsed_ms="));
    assert!(stdout.contains("throughput_mib_s="));
    assert!(!stdout.contains("relay_turns="));
}

#[test]
fn ktp_e2e_bench_cli_rejects_zero_clients() {
    let exe = std::env::var("CARGO_BIN_EXE_ktp-e2e-bench")
        .expect("ktp-e2e-bench binary should be built by cargo");

    let output = Command::new(exe)
        .args(["--clients", "0"])
        .output()
        .expect("ktp-e2e-bench should run");

    assert!(
        !output.status.success(),
        "ktp-e2e-bench unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--clients must be greater than zero"));
}

#[test]
fn ktp_e2e_bench_cli_rejects_zero_runs() {
    let exe = std::env::var("CARGO_BIN_EXE_ktp-e2e-bench")
        .expect("ktp-e2e-bench binary should be built by cargo");

    let output = Command::new(exe)
        .args(["--runs", "0"])
        .output()
        .expect("ktp-e2e-bench should run");

    assert!(
        !output.status.success(),
        "ktp-e2e-bench unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--runs must be greater than zero"));
}

#[test]
fn ktp_e2e_bench_cli_reports_multi_client_aggregate_throughput() {
    let exe = std::env::var("CARGO_BIN_EXE_ktp-e2e-bench")
        .expect("ktp-e2e-bench binary should be built by cargo");

    let output = Command::new(exe)
        .args(["--clients", "2", "--frames", "2", "--payload-bytes", "128"])
        .output()
        .expect("ktp-e2e-bench should run");

    assert!(
        output.status.success(),
        "ktp-e2e-bench failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("clients=2"));
    assert!(stdout.contains("frames=2"));
    assert!(stdout.contains("payload_bytes=128"));
    assert!(stdout.contains("bytes=512"));
    assert!(stdout.contains("throughput_mib_s="));
}

#[test]
fn ktp_e2e_bench_cli_reports_repeated_run_statistics() {
    let exe = std::env::var("CARGO_BIN_EXE_ktp-e2e-bench")
        .expect("ktp-e2e-bench binary should be built by cargo");

    let output = Command::new(exe)
        .args([
            "--runs",
            "2",
            "--clients",
            "2",
            "--frames",
            "2",
            "--payload-bytes",
            "128",
        ])
        .output()
        .expect("ktp-e2e-bench should run");

    assert!(
        output.status.success(),
        "ktp-e2e-bench failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("runs=2"));
    assert!(stdout.contains("clients=2"));
    assert!(stdout.contains("bytes=512"));
    assert!(stdout.contains("elapsed_ms_min="));
    assert!(stdout.contains("elapsed_ms_median="));
    assert!(stdout.contains("elapsed_ms_max="));
    assert!(stdout.contains("throughput_mib_s_min="));
    assert!(stdout.contains("throughput_mib_s_median="));
    assert!(stdout.contains("throughput_mib_s_max="));
}

#[test]
fn ktp_e2e_bench_cli_reports_relay_diagnostics_when_requested() {
    let exe = std::env::var("CARGO_BIN_EXE_ktp-e2e-bench")
        .expect("ktp-e2e-bench binary should be built by cargo");

    let output = Command::new(exe)
        .args([
            "--diagnostics",
            "--relay-wait-timeout-us",
            "100",
            "--clients",
            "2",
            "--frames",
            "2",
            "--payload-bytes",
            "128",
        ])
        .output()
        .expect("ktp-e2e-bench should run");

    assert!(
        output.status.success(),
        "ktp-e2e-bench failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("relay_turns="));
    assert!(stdout.contains("relay_empty_turns="));
    assert!(stdout.contains("relay_yield_turns="));
    assert!(stdout.contains("relay_wait_turns="));
    assert!(stdout.contains("ingress_frames="));
    assert!(stdout.contains("egress_frames="));
    assert!(stdout.contains("ingress_data_frames="));
    assert!(stdout.contains("egress_data_frames="));
    assert!(stdout.contains("ingress_batches="));
    assert!(stdout.contains("egress_batches="));
    assert!(stdout.contains("ingress_max_batch_frames="));
    assert!(stdout.contains("egress_max_batch_frames="));
}

#[test]
fn ktp_e2e_bench_cli_reports_latency_percentiles_when_requested() {
    let exe = std::env::var("CARGO_BIN_EXE_ktp-e2e-bench")
        .expect("ktp-e2e-bench binary should be built by cargo");

    let output = Command::new(exe)
        .args([
            "--latency",
            "--clients",
            "2",
            "--frames",
            "2",
            "--payload-bytes",
            "128",
        ])
        .output()
        .expect("ktp-e2e-bench should run");

    assert!(
        output.status.success(),
        "ktp-e2e-bench failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("rtt_micros_p50="));
    assert!(stdout.contains("rtt_micros_p95="));
    assert!(stdout.contains("rtt_micros_p99="));
    assert!(stdout.contains("rtt_micros_max="));
}

#[test]
fn ktp_e2e_bench_cli_reports_rdp_like_profile_metrics() {
    let exe = std::env::var("CARGO_BIN_EXE_ktp-e2e-bench")
        .expect("ktp-e2e-bench binary should be built by cargo");

    let output = Command::new(exe)
        .args([
            "--profile",
            "rdp-like",
            "--latency",
            "--diagnostics",
            "--relay-wait-timeout-us",
            "100",
            "--clients",
            "1",
            "--frames",
            "13",
            "--payload-bytes",
            "1024",
        ])
        .output()
        .expect("ktp-e2e-bench should run");

    assert!(
        output.status.success(),
        "ktp-e2e-bench failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("profile=rdp_like"));
    assert!(stdout.contains("frames=13"));
    assert!(stdout.contains("payload_bytes=1024"));
    assert!(stdout.contains("bytes=3904"));
    assert!(stdout.contains("rtt_micros_samples=13"));
    assert!(stdout.contains("relay_turns="));
}

#[test]
fn ktp_e2e_bench_cli_rejects_unknown_profile() {
    let exe = std::env::var("CARGO_BIN_EXE_ktp-e2e-bench")
        .expect("ktp-e2e-bench binary should be built by cargo");

    let output = Command::new(exe)
        .args(["--profile", "udp"])
        .output()
        .expect("ktp-e2e-bench should run");

    assert!(
        !output.status.success(),
        "ktp-e2e-bench unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--profile must be fixed or rdp-like"));
}
