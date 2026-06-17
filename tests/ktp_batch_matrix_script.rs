use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

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
    assert!(script.contains("KTP_BATCH_MATRIX_BATCH_POLICY"));
    assert!(script.contains("KTP_BATCH_MATRIX_BATCH_POLICIES"));
    assert!(script.contains("--relay-batch-policy"));
    assert!(script.contains("relay_batch_frames=$batch"));
    assert!(script.contains("KTP_BATCH_MATRIX_CSV"));
    assert!(script.contains("write_csv_row"));
    assert!(script.contains("throughput_mib_s_median"));
    assert!(script.contains("rtt_client_p95_spread_micros"));
    assert!(script.contains("relay_batch_frames_effective"));
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

#[test]
fn ktp_batch_matrix_script_dry_run_expands_each_client_and_batch_on_linux() {
    if !cfg!(target_os = "linux") || Command::new("bash").arg("--version").output().is_err() {
        return;
    }

    let output = Command::new("bash")
        .env("KTP_BATCH_MATRIX_DRY_RUN", "1")
        .env("KTP_BATCH_MATRIX_CLIENTS", "1 4")
        .env("KTP_BATCH_MATRIX_BATCHES", "16 32")
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
    assert_eq!(stdout.matches("dry_run:").count(), 4);
    assert!(stdout.contains("clients=1 relay_batch_frames=16"));
    assert!(stdout.contains("clients=1 relay_batch_frames=32"));
    assert!(stdout.contains("clients=4 relay_batch_frames=16"));
    assert!(stdout.contains("clients=4 relay_batch_frames=32"));
    assert!(stdout.contains("--clients 1"));
    assert!(stdout.contains("--clients 4"));
    assert!(!stdout.contains("--clients '1 4'"));
}

#[test]
fn ktp_batch_matrix_script_dry_run_expands_each_policy_on_linux() {
    if !cfg!(target_os = "linux") || Command::new("bash").arg("--version").output().is_err() {
        return;
    }

    let output = Command::new("bash")
        .env("KTP_BATCH_MATRIX_DRY_RUN", "1")
        .env("KTP_BATCH_MATRIX_BATCH_POLICIES", "fixed adaptive")
        .env("KTP_BATCH_MATRIX_CLIENTS", "4")
        .env("KTP_BATCH_MATRIX_BATCHES", "64")
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
    assert_eq!(stdout.matches("dry_run:").count(), 2);
    assert!(stdout.contains("relay_batch_policy=fixed"));
    assert!(stdout.contains("relay_batch_policy=adaptive"));
    assert!(stdout.contains("--relay-batch-policy fixed"));
    assert!(stdout.contains("--relay-batch-policy adaptive"));
}

#[test]
fn ktp_batch_matrix_script_dry_run_does_not_create_csv_on_linux() {
    if !cfg!(target_os = "linux") || Command::new("bash").arg("--version").output().is_err() {
        return;
    }

    let csv_path = unique_temp_path("ktp-batch-matrix-dry-run", "csv");
    let _ = std::fs::remove_file(&csv_path);
    let output = Command::new("bash")
        .env("KTP_BATCH_MATRIX_DRY_RUN", "1")
        .env("KTP_BATCH_MATRIX_BATCHES", "1 4")
        .env("KTP_BATCH_MATRIX_CSV", &csv_path)
        .args(["scripts/ktp-relay-batch-matrix.sh"])
        .output()
        .expect("batch matrix dry-run should run");

    assert!(
        output.status.success(),
        "batch matrix dry-run failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !csv_path.exists(),
        "dry-run should not create a CSV file at {}",
        csv_path.display()
    );
}

#[test]
fn ktp_batch_matrix_script_writes_csv_for_each_policy_on_linux() {
    if !cfg!(target_os = "linux") || Command::new("bash").arg("--version").output().is_err() {
        return;
    }

    let fake_bin_dir = unique_temp_path("ktp-policy-matrix-fake-bin", "");
    let _ = std::fs::remove_dir_all(&fake_bin_dir);
    std::fs::create_dir_all(&fake_bin_dir).expect("fake bin dir should be created");
    let fake_cargo = fake_bin_dir.join("cargo");
    std::fs::write(&fake_cargo, fake_cargo_script()).expect("fake cargo should be written");
    let chmod_status = Command::new("chmod")
        .args([
            "+x",
            fake_cargo
                .to_str()
                .expect("fake cargo path should be utf-8"),
        ])
        .status()
        .expect("chmod should run");
    assert!(chmod_status.success());

    let csv_path = unique_temp_path("ktp-policy-matrix", "csv");
    let _ = std::fs::remove_file(&csv_path);
    let original_path = std::env::var("PATH").expect("PATH should be set");
    let test_path = format!("{}:{original_path}", fake_bin_dir.display());
    let output = Command::new("bash")
        .env("PATH", test_path)
        .env("KTP_BATCH_MATRIX_BATCH_POLICIES", "fixed adaptive")
        .env("KTP_BATCH_MATRIX_BATCHES", "64")
        .env("KTP_BATCH_MATRIX_RUNS", "1")
        .env("KTP_BATCH_MATRIX_CLIENTS", "4")
        .env("KTP_BATCH_MATRIX_FRAMES", "8")
        .env("KTP_BATCH_MATRIX_PAYLOAD_BYTES", "1024")
        .env("KTP_BATCH_MATRIX_CSV", &csv_path)
        .args(["scripts/ktp-relay-batch-matrix.sh"])
        .output()
        .expect("batch matrix should run with fake cargo");

    assert!(
        output.status.success(),
        "batch matrix failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let csv = std::fs::read_to_string(&csv_path).expect("CSV should be written");
    assert!(csv.contains(
        "rdp-like,1,4,8,1024,64,fixed,64,64.000,64.000,64.000,64.500,64.500,64.500,10,20,30,40,20,20,0,40,7,2,3,4,64,64"
    ));
    assert!(csv.contains(
        "rdp-like,1,4,8,1024,64,adaptive,32,64.000,64.000,64.000,64.500,64.500,64.500,10,20,30,40,20,20,0,40,7,2,3,4,32,32"
    ));
}

#[test]
fn ktp_batch_matrix_script_writes_csv_from_bench_output_on_linux() {
    if !cfg!(target_os = "linux") || Command::new("bash").arg("--version").output().is_err() {
        return;
    }

    let fake_bin_dir = unique_temp_path("ktp-batch-matrix-fake-bin", "");
    let _ = std::fs::remove_dir_all(&fake_bin_dir);
    std::fs::create_dir_all(&fake_bin_dir).expect("fake bin dir should be created");
    let fake_cargo = fake_bin_dir.join("cargo");
    std::fs::write(&fake_cargo, fake_cargo_script()).expect("fake cargo should be written");
    let chmod_status = Command::new("chmod")
        .args([
            "+x",
            fake_cargo
                .to_str()
                .expect("fake cargo path should be utf-8"),
        ])
        .status()
        .expect("chmod should run");
    assert!(chmod_status.success());

    let csv_path = unique_temp_path("ktp-batch-matrix", "csv");
    let _ = std::fs::remove_file(&csv_path);
    let original_path = std::env::var("PATH").expect("PATH should be set");
    let test_path = format!("{}:{original_path}", fake_bin_dir.display());
    let output = Command::new("bash")
        .env("PATH", test_path)
        .env("KTP_BATCH_MATRIX_BATCHES", "1 4")
        .env("KTP_BATCH_MATRIX_RUNS", "1")
        .env("KTP_BATCH_MATRIX_CLIENTS", "1")
        .env("KTP_BATCH_MATRIX_FRAMES", "8")
        .env("KTP_BATCH_MATRIX_PAYLOAD_BYTES", "1024")
        .env("KTP_BATCH_MATRIX_CSV", &csv_path)
        .args(["scripts/ktp-relay-batch-matrix.sh"])
        .output()
        .expect("batch matrix should run with fake cargo");

    assert!(
        output.status.success(),
        "batch matrix failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let csv = std::fs::read_to_string(&csv_path).expect("CSV should be written");
    assert!(csv.contains("profile,runs,clients,frames,payload_bytes,relay_batch_frames"));
    assert!(csv.contains("relay_batch_policy,relay_batch_frames_effective"));
    assert!(csv.contains(
        "rtt_client_p95_micros_min,rtt_client_p95_micros_max,rtt_client_p95_spread_micros,rtt_client_max_micros_max"
    ));
    assert!(csv.contains(
        "rdp-like,1,1,8,1024,1,fixed,1,1.000,1.000,1.000,1.500,1.500,1.500,10,20,30,40,20,20,0,40,7,2,3,4,1,1"
    ));
    assert!(csv.contains(
        "rdp-like,1,1,8,1024,4,fixed,4,4.000,4.000,4.000,4.500,4.500,4.500,10,20,30,40,20,20,0,40,7,2,3,4,4,4"
    ));
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
batch=""
clients="1"
policy="fixed"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --clients)
      clients="$2"
      shift 2
      ;;
    --relay-batch-frames)
      batch="$2"
      shift 2
      ;;
    --relay-batch-policy)
      policy="$2"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done
if [[ -z "$batch" ]]; then
  echo "missing --relay-batch-frames" >&2
  exit 9
fi
effective="$batch"
if [[ "$policy" == "adaptive" && "$clients" -ge 8 && "$effective" -gt 16 ]]; then
  effective="16"
elif [[ "$policy" == "adaptive" && "$clients" -ge 4 && "$effective" -gt 32 ]]; then
  effective="32"
fi
echo "ktp_e2e_bench mode=runtime_ingress_egress transport=ktp_tcp bridge=batch profile=rdp_like runs=1 clients=${clients} frames=8 payload_bytes=1024 bytes=1472 elapsed_ms=${batch}.000 throughput_mib_s=${batch}.500 rtt_micros_samples=8 rtt_micros_p50=10 rtt_micros_p95=20 rtt_micros_p99=30 rtt_micros_max=40 rtt_client_p95_micros_min=20 rtt_client_p95_micros_max=20 rtt_client_p95_spread_micros=0 rtt_client_max_micros_max=40 relay_batch_policy=${policy} relay_batch_frames=${batch} relay_batch_frames_effective=${effective} relay_turns=7 relay_empty_turns=0 relay_yield_turns=6 relay_wait_turns=2 ingress_frames=9 egress_frames=8 ingress_data_frames=8 egress_data_frames=8 ingress_batches=3 egress_batches=4 ingress_max_batch_frames=${effective} egress_max_batch_frames=${effective}"
"#
}
