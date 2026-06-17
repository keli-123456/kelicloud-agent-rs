use std::process::Command;

#[test]
fn ktp_tunnel_bench_cli_reports_loopback_throughput() {
    let exe = std::env::var("CARGO_BIN_EXE_ktp-tunnel-bench")
        .expect("ktp-tunnel-bench binary should be built by cargo");

    let output = Command::new(exe)
        .args(["--frames", "4", "--payload-bytes", "128"])
        .output()
        .expect("ktp-tunnel-bench should run");

    assert!(
        output.status.success(),
        "ktp-tunnel-bench failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ktp_tunnel_bench"));
    assert!(stdout.contains("frames=4"));
    assert!(stdout.contains("bytes=512"));
    assert!(stdout.contains("elapsed_ms="));
    assert!(stdout.contains("throughput_mib_s="));
}

#[test]
fn ktp_tunnel_bench_cli_can_average_multiple_runs() {
    let exe = std::env::var("CARGO_BIN_EXE_ktp-tunnel-bench")
        .expect("ktp-tunnel-bench binary should be built by cargo");

    let output = Command::new(exe)
        .args(["--frames", "2", "--payload-bytes", "128", "--runs", "2"])
        .output()
        .expect("ktp-tunnel-bench should run");

    assert!(
        output.status.success(),
        "ktp-tunnel-bench failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("runs=2"));
    assert!(stdout.contains("frames=2"));
    assert!(stdout.contains("bytes_per_run=256"));
    assert!(stdout.contains("total_bytes=512"));
    assert!(stdout.contains("elapsed_ms_min="));
    assert!(stdout.contains("elapsed_ms_median="));
    assert!(stdout.contains("elapsed_ms_max="));
    assert!(stdout.contains("throughput_mib_s_min="));
    assert!(stdout.contains("throughput_mib_s_median="));
    assert!(stdout.contains("throughput_mib_s_max="));
}

#[test]
fn ktp_tunnel_bench_cli_reports_relay_to_client_batch_read() {
    let exe = std::env::var("CARGO_BIN_EXE_ktp-tunnel-bench")
        .expect("ktp-tunnel-bench binary should be built by cargo");

    let output = Command::new(exe)
        .args([
            "--direction",
            "relay-to-client-batch-read",
            "--frames",
            "4",
            "--payload-bytes",
            "128",
        ])
        .output()
        .expect("ktp-tunnel-bench should run");

    assert!(
        output.status.success(),
        "ktp-tunnel-bench failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ktp_tunnel_bench"));
    assert!(stdout.contains("direction=relay_to_client_batch_read"));
    assert!(stdout.contains("read_batch_frames=64"));
    assert!(stdout.contains("frames=4"));
    assert!(stdout.contains("bytes=512"));
    assert!(stdout.contains("elapsed_ms="));
    assert!(stdout.contains("throughput_mib_s="));
}

#[test]
fn ktp_tunnel_bench_cli_reports_client_to_relay_batch_write() {
    let exe = std::env::var("CARGO_BIN_EXE_ktp-tunnel-bench")
        .expect("ktp-tunnel-bench binary should be built by cargo");

    let output = Command::new(exe)
        .args([
            "--direction",
            "client-to-relay-batch-write",
            "--frames",
            "4",
            "--payload-bytes",
            "128",
        ])
        .output()
        .expect("ktp-tunnel-bench should run");

    assert!(
        output.status.success(),
        "ktp-tunnel-bench failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ktp_tunnel_bench"));
    assert!(stdout.contains("direction=client_to_relay_batch_write"));
    assert!(stdout.contains("write_batch_frames=64"));
    assert!(stdout.contains("write_batch_reused=1"));
    assert!(stdout.contains("frames=4"));
    assert!(stdout.contains("bytes=512"));
    assert!(stdout.contains("elapsed_ms="));
    assert!(stdout.contains("throughput_mib_s="));
}

#[test]
fn tunnel_relay_smoke_script_runs_ktp_tunnel_bench() {
    let script = std::fs::read_to_string("scripts/tunnel-relay-local-smoke.sh")
        .expect("smoke script should be readable");

    assert!(script.contains("cargo run --bin ktp-tunnel-bench"));
    assert!(script.contains("KTP_SMOKE_CARRIER_RUNS"));
    assert!(script.contains("--frames 4096"));
    assert!(script.contains("--payload-bytes 16384"));
    assert!(script.contains("--runs \"${KTP_SMOKE_CARRIER_RUNS}\""));
    assert!(script.contains("KTP_SMOKE_BATCH_READ_FRAMES"));
    assert!(script.contains("--direction relay-to-client-batch-read"));
}
