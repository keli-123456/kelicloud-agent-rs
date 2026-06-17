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
    assert!(stdout.contains("frames=3"));
    assert!(stdout.contains("payload_bytes=128"));
    assert!(stdout.contains("bytes=384"));
    assert!(stdout.contains("elapsed_ms="));
    assert!(stdout.contains("throughput_mib_s="));
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
