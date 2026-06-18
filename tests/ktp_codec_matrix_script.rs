use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn ktp_codec_matrix_script_sweeps_modes_with_repeatable_defaults() {
    let script = std::fs::read_to_string("scripts/ktp-codec-matrix.sh")
        .expect("codec matrix script should be readable");

    assert!(script.contains("KTP_CODEC_MATRIX_MODES"));
    assert!(script.contains("stream crypto"));
    assert!(script.contains("KTP_CODEC_MATRIX_FRAMES"));
    assert!(script.contains("KTP_CODEC_MATRIX_PAYLOAD_BYTES"));
    assert!(script.contains("KTP_CODEC_MATRIX_CHUNK_FRAMES"));
    assert!(script.contains("KTP_CODEC_MATRIX_CSV"));
    assert!(script.contains("cargo run --release --bin ktp-codec-bench"));
    assert!(script.contains("--mode"));
    assert!(script.contains("--runs"));
    assert!(script.contains("--frames"));
    assert!(script.contains("--payload-bytes"));
    assert!(script.contains("--chunk-frames"));
    assert!(script.contains("write_csv_row"));
    assert!(script.contains("cursor_compaction"));
    assert!(script.contains("throughput_mib_s_median"));
}

#[test]
fn ktp_codec_matrix_script_has_valid_bash_syntax_when_bash_is_available() {
    if Command::new("bash").arg("--version").output().is_err() {
        return;
    }

    let status = Command::new("bash")
        .args(["-n", "scripts/ktp-codec-matrix.sh"])
        .status()
        .expect("bash -n should run");

    assert!(status.success());
}

#[test]
fn ktp_codec_matrix_script_dry_run_expands_modes_and_payloads_on_linux() {
    if !cfg!(target_os = "linux") || Command::new("bash").arg("--version").output().is_err() {
        return;
    }

    let output = Command::new("bash")
        .env("KTP_CODEC_MATRIX_DRY_RUN", "1")
        .env("KTP_CODEC_MATRIX_MODES", "stream crypto")
        .env("KTP_CODEC_MATRIX_FRAMES", "8")
        .env("KTP_CODEC_MATRIX_PAYLOAD_BYTES", "512 1024")
        .env("KTP_CODEC_MATRIX_CHUNK_FRAMES", "4")
        .env("KTP_CODEC_MATRIX_RUNS", "2")
        .args(["scripts/ktp-codec-matrix.sh"])
        .output()
        .expect("codec matrix dry-run should run");

    assert!(
        output.status.success(),
        "codec matrix dry-run failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.matches("dry_run:").count(), 4);
    assert!(stdout.contains("mode=stream frames=8 payload_bytes=512 chunk_frames=4"));
    assert!(stdout.contains("mode=stream frames=8 payload_bytes=1024 chunk_frames=4"));
    assert!(stdout.contains("mode=crypto frames=8 payload_bytes=512 chunk_frames=4"));
    assert!(stdout.contains("mode=crypto frames=8 payload_bytes=1024 chunk_frames=4"));
    assert!(stdout.contains("--mode stream"));
    assert!(stdout.contains("--mode crypto"));
    assert!(stdout.contains("--runs 2"));
}

#[test]
fn ktp_codec_matrix_script_writes_csv_from_bench_output_on_linux() {
    if !cfg!(target_os = "linux") || Command::new("bash").arg("--version").output().is_err() {
        return;
    }

    let fake_bin_dir = unique_temp_path("ktp-codec-matrix-fake-bin", "");
    let _ = std::fs::remove_dir_all(&fake_bin_dir);
    std::fs::create_dir_all(&fake_bin_dir).expect("fake bin dir should be created");
    let fake_cargo = fake_bin_dir.join("cargo");
    std::fs::write(&fake_cargo, fake_cargo_script()).expect("fake cargo should be written");
    let chmod_status = Command::new("chmod")
        .args(["+x", fake_cargo.to_str().expect("fake cargo path")])
        .status()
        .expect("chmod should run");
    assert!(chmod_status.success());

    let csv_path = unique_temp_path("ktp-codec-matrix", "csv");
    let _ = std::fs::remove_file(&csv_path);
    let original_path = std::env::var("PATH").expect("PATH should be set");
    let test_path = format!("{}:{original_path}", fake_bin_dir.display());
    let output = Command::new("bash")
        .env("PATH", test_path)
        .env("KTP_CODEC_MATRIX_MODES", "stream crypto")
        .env("KTP_CODEC_MATRIX_FRAMES", "8")
        .env("KTP_CODEC_MATRIX_PAYLOAD_BYTES", "1024")
        .env("KTP_CODEC_MATRIX_CHUNK_FRAMES", "4")
        .env("KTP_CODEC_MATRIX_RUNS", "3")
        .env("KTP_CODEC_MATRIX_CSV", &csv_path)
        .args(["scripts/ktp-codec-matrix.sh"])
        .output()
        .expect("codec matrix should run with fake cargo");

    assert!(
        output.status.success(),
        "codec matrix failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let csv = std::fs::read_to_string(&csv_path).expect("CSV should be written");
    assert!(csv.contains(
        "mode,runs,frames,payload_bytes,chunk_frames,bytes_per_run,total_bytes,cursor_compaction,elapsed_ms_min,elapsed_ms_median,elapsed_ms_max,throughput_mib_s_min,throughput_mib_s_median,throughput_mib_s_max"
    ));
    assert!(csv.contains("stream,3,8,1024,4,8192,24576,1,8.100,8.200,8.300,8.400,8.500,8.600"));
    assert!(csv.contains("crypto,3,8,1024,4,8192,24576,1,8.100,8.200,8.300,8.400,8.500,8.600"));
}

fn unique_temp_path(prefix: &str, extension: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    let mut name = format!("{prefix}-{}-{nanos}", std::process::id());
    if !extension.is_empty() {
        name.push('.');
        name.push_str(extension);
    }
    std::env::temp_dir().join(name)
}

fn fake_cargo_script() -> &'static str {
    r#"#!/usr/bin/env bash
set -euo pipefail
bin=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --bin)
      bin="$2"
      shift 2
      ;;
    --)
      shift
      break
      ;;
    *)
      shift
      ;;
  esac
done
if [[ "$bin" != "ktp-codec-bench" ]]; then
  echo "unexpected fake cargo bin: $bin" >&2
  exit 9
fi
mode="stream"
frames="0"
payload_bytes="0"
chunk_frames="0"
runs="1"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode)
      mode="$2"
      shift 2
      ;;
    --frames)
      frames="$2"
      shift 2
      ;;
    --payload-bytes)
      payload_bytes="$2"
      shift 2
      ;;
    --chunk-frames)
      chunk_frames="$2"
      shift 2
      ;;
    --runs)
      runs="$2"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done
echo "ktp_codec_bench mode=${mode} runs=${runs} frames=${frames} payload_bytes=${payload_bytes} chunk_frames=${chunk_frames} bytes=8192 bytes_per_run=8192 total_bytes=24576 cursor_compaction=1 elapsed_ms_min=${frames}.100 elapsed_ms_median=${frames}.200 elapsed_ms_max=${frames}.300 throughput_mib_s_min=${frames}.400 throughput_mib_s_median=${frames}.500 throughput_mib_s_max=${frames}.600"
"#
}
