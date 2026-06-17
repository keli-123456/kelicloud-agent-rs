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
    assert!(stdout.contains("frames=3"));
    assert!(stdout.contains("payload_bytes=128"));
    assert!(stdout.contains("bytes=384"));
    assert!(stdout.contains("elapsed_ms="));
    assert!(stdout.contains("throughput_mib_s="));
}
