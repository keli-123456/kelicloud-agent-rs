use std::process::Command;

#[test]
fn ktp_codec_bench_cli_reports_stream_cursor_decode_throughput() {
    let exe = std::env::var("CARGO_BIN_EXE_ktp-codec-bench")
        .expect("ktp-codec-bench binary should be built by cargo");

    let output = Command::new(exe)
        .args([
            "--mode",
            "stream",
            "--frames",
            "8",
            "--payload-bytes",
            "128",
            "--chunk-frames",
            "4",
        ])
        .output()
        .expect("ktp-codec-bench should run");

    assert!(
        output.status.success(),
        "ktp-codec-bench failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ktp_codec_bench"));
    assert!(stdout.contains("mode=stream"));
    assert!(stdout.contains("frames=8"));
    assert!(stdout.contains("payload_bytes=128"));
    assert!(stdout.contains("chunk_frames=4"));
    assert!(stdout.contains("bytes=1024"));
    assert!(stdout.contains("cursor_compaction=1"));
    assert!(stdout.contains("elapsed_ms="));
    assert!(stdout.contains("throughput_mib_s="));
}

#[test]
fn ktp_codec_bench_cli_reports_crypto_record_cursor_decode_throughput() {
    let exe = std::env::var("CARGO_BIN_EXE_ktp-codec-bench")
        .expect("ktp-codec-bench binary should be built by cargo");

    let output = Command::new(exe)
        .args([
            "--mode",
            "crypto",
            "--frames",
            "8",
            "--payload-bytes",
            "128",
            "--chunk-frames",
            "4",
            "--runs",
            "2",
        ])
        .output()
        .expect("ktp-codec-bench should run");

    assert!(
        output.status.success(),
        "ktp-codec-bench failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ktp_codec_bench"));
    assert!(stdout.contains("mode=crypto"));
    assert!(stdout.contains("runs=2"));
    assert!(stdout.contains("frames=8"));
    assert!(stdout.contains("payload_bytes=128"));
    assert!(stdout.contains("chunk_frames=4"));
    assert!(stdout.contains("bytes_per_run=1024"));
    assert!(stdout.contains("total_bytes=2048"));
    assert!(stdout.contains("cursor_compaction=1"));
    assert!(stdout.contains("elapsed_ms_min="));
    assert!(stdout.contains("elapsed_ms_median="));
    assert!(stdout.contains("elapsed_ms_max="));
    assert!(stdout.contains("throughput_mib_s_min="));
    assert!(stdout.contains("throughput_mib_s_median="));
    assert!(stdout.contains("throughput_mib_s_max="));
}
