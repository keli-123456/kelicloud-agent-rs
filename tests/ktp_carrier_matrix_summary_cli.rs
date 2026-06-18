use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn ktp_carrier_matrix_summary_accepts_reused_batch_positive_throughput() {
    let csv_path = write_temp_csv(
        "ktp-carrier-matrix-summary-pass",
        r#"carrier,crypto,direction,runs,frames,payload_bytes,write_batch_frames,write_batch_reused,read_batch_frames,read_batch_reused,elapsed_ms_min,elapsed_ms_median,elapsed_ms_max,throughput_mib_s_min,throughput_mib_s_median,throughput_mib_s_max
ktp_tcp,ktp_aead,client_to_relay,3,256,1024,0,0,0,0,20.0,21.0,22.0,11.0,12.0,13.0
ktp_tcp,ktp_aead,client_to_relay_batch_write,3,256,1024,64,1,0,0,10.0,11.0,12.0,21.0,22.0,23.0
ktp_tcp,ktp_aead,relay_to_client_batch_read,3,256,1024,0,0,64,1,9.0,10.0,11.0,31.0,32.0,33.0
"#,
    );

    let output = Command::new(summary_exe())
        .arg("--require-ktp-aead")
        .arg("--require-batch-reuse")
        .arg("--require-positive-throughput")
        .arg(&csv_path)
        .output()
        .expect("ktp-carrier-matrix-summary should run");

    assert!(
        output.status.success(),
        "summary failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ktp_carrier_matrix_summary rows=3 gate=pass"));
    assert!(stdout.contains("batch_write_throughput_mib_s_median=22.000"));
    assert!(stdout.contains("batch_read_throughput_mib_s_median=32.000"));
}

#[test]
fn ktp_carrier_matrix_summary_rejects_missing_batch_reuse_evidence() {
    let csv_path = write_temp_csv(
        "ktp-carrier-matrix-summary-no-reuse",
        r#"carrier,crypto,direction,runs,frames,payload_bytes,write_batch_frames,write_batch_reused,read_batch_frames,read_batch_reused,elapsed_ms_min,elapsed_ms_median,elapsed_ms_max,throughput_mib_s_min,throughput_mib_s_median,throughput_mib_s_max
ktp_tcp,ktp_aead,client_to_relay,3,256,1024,0,0,0,0,20.0,21.0,22.0,11.0,12.0,13.0
ktp_tcp,ktp_aead,client_to_relay_batch_write,3,256,1024,64,0,0,0,10.0,11.0,12.0,21.0,22.0,23.0
ktp_tcp,ktp_aead,relay_to_client_batch_read,3,256,1024,0,0,64,1,9.0,10.0,11.0,31.0,32.0,33.0
"#,
    );

    let output = Command::new(summary_exe())
        .arg("--require-batch-reuse")
        .arg(&csv_path)
        .output()
        .expect("ktp-carrier-matrix-summary should run");

    assert_eq!(
        output.status.code(),
        Some(3),
        "missing reuse should exit 3: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("client_to_relay_batch_write write_batch_reused is not 1"));
}

#[test]
fn ktp_carrier_matrix_summary_rejects_zero_batch_throughput() {
    let csv_path = write_temp_csv(
        "ktp-carrier-matrix-summary-zero-throughput",
        r#"carrier,crypto,direction,runs,frames,payload_bytes,write_batch_frames,write_batch_reused,read_batch_frames,read_batch_reused,elapsed_ms_min,elapsed_ms_median,elapsed_ms_max,throughput_mib_s_min,throughput_mib_s_median,throughput_mib_s_max
ktp_tcp,ktp_aead,client_to_relay,3,256,1024,0,0,0,0,20.0,21.0,22.0,11.0,12.0,13.0
ktp_tcp,ktp_aead,client_to_relay_batch_write,3,256,1024,64,1,0,0,10.0,11.0,12.0,21.0,0.0,23.0
ktp_tcp,ktp_aead,relay_to_client_batch_read,3,256,1024,0,0,64,1,9.0,10.0,11.0,31.0,32.0,33.0
"#,
    );

    let output = Command::new(summary_exe())
        .arg("--require-positive-throughput")
        .arg(&csv_path)
        .output()
        .expect("ktp-carrier-matrix-summary should run");

    assert_eq!(
        output.status.code(),
        Some(3),
        "zero throughput should exit 3: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("client_to_relay_batch_write throughput_mib_s_median must be positive"));
}

#[test]
fn ktp_carrier_matrix_summary_rejects_batch_write_below_threshold() {
    let csv_path = write_temp_csv(
        "ktp-carrier-matrix-summary-low-write",
        r#"carrier,crypto,direction,runs,frames,payload_bytes,write_batch_frames,write_batch_reused,read_batch_frames,read_batch_reused,elapsed_ms_min,elapsed_ms_median,elapsed_ms_max,throughput_mib_s_min,throughput_mib_s_median,throughput_mib_s_max
ktp_tcp,ktp_aead,client_to_relay,3,256,1024,0,0,0,0,20.0,21.0,22.0,11.0,12.0,13.0
ktp_tcp,ktp_aead,client_to_relay_batch_write,3,256,1024,64,1,0,0,10.0,11.0,12.0,21.0,22.0,23.0
ktp_tcp,ktp_aead,relay_to_client_batch_read,3,256,1024,0,0,64,1,9.0,10.0,11.0,31.0,32.0,33.0
"#,
    );

    let output = Command::new(summary_exe())
        .arg("--min-batch-write-throughput-mib-s")
        .arg("30")
        .arg(&csv_path)
        .output()
        .expect("ktp-carrier-matrix-summary should run");

    assert_eq!(
        output.status.code(),
        Some(3),
        "low batch-write throughput should exit 3: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr
        .contains("client_to_relay_batch_write throughput_mib_s_median 22.000 below min 30.000"));
}

fn summary_exe() -> String {
    std::env::var("CARGO_BIN_EXE_ktp-carrier-matrix-summary")
        .expect("ktp-carrier-matrix-summary binary should be built by cargo")
}

fn write_temp_csv(prefix: &str, content: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}.csv", std::process::id()));
    std::fs::write(&path, content).expect("temporary CSV should be written");
    path
}
