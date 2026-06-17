use std::process::Command;

#[test]
fn ktp_batch_matrix_script_sweeps_relay_batch_frames_with_rdp_like_defaults() {
    let script = std::fs::read_to_string("scripts/ktp-relay-batch-matrix.sh")
        .expect("batch matrix script should be readable");

    assert!(script.contains("KTP_BATCH_MATRIX_BATCHES:-1 2 4 8 16 32 64"));
    assert!(script.contains("cargo run --release --bin ktp-e2e-bench"));
    assert!(script.contains("--profile"));
    assert!(script.contains("--diagnostics"));
    assert!(script.contains("--latency"));
    assert!(script.contains("--relay-wait-timeout-us"));
    assert!(script.contains("--relay-batch-frames"));
    assert!(script.contains("relay_batch_frames=$batch"));
}

#[test]
fn ktp_batch_matrix_script_has_valid_bash_syntax_when_bash_is_available() {
    if Command::new("bash").arg("--version").output().is_err() {
        return;
    }

    let status = Command::new("bash")
        .args(["-n", "scripts/ktp-relay-batch-matrix.sh"])
        .status()
        .expect("bash -n should run");

    assert!(status.success());
}

#[test]
fn ktp_batch_matrix_script_dry_run_expands_each_batch_on_linux() {
    if !cfg!(target_os = "linux") || Command::new("bash").arg("--version").output().is_err() {
        return;
    }

    let output = Command::new("bash")
        .env("KTP_BATCH_MATRIX_DRY_RUN", "1")
        .env("KTP_BATCH_MATRIX_BATCHES", "1 4")
        .env("KTP_BATCH_MATRIX_RUNS", "2")
        .env("KTP_BATCH_MATRIX_FRAMES", "8")
        .env("KTP_BATCH_MATRIX_PAYLOAD_BYTES", "1024")
        .args(["scripts/ktp-relay-batch-matrix.sh"])
        .output()
        .expect("batch matrix dry-run should run");

    assert!(
        output.status.success(),
        "batch matrix dry-run failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("relay_batch_frames=1"));
    assert!(stdout.contains("relay_batch_frames=4"));
    assert!(stdout.contains("--relay-batch-frames 1"));
    assert!(stdout.contains("--relay-batch-frames 4"));
    assert!(stdout.contains("--runs 2"));
    assert!(stdout.contains("--frames 8"));
    assert!(stdout.contains("--payload-bytes 1024"));
}
