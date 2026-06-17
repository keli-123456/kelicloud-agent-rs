use std::path::PathBuf;
use std::process::Command;

#[test]
fn ktp_local_backend_matrix_script_declares_carrier_matrix_contract() {
    let script = std::fs::read_to_string(script_path())
        .expect("local backend carrier matrix script should be readable");

    assert!(script.contains("KELICLOUD_LOCAL_BACKEND_MATRIX_CARRIERS"));
    assert!(script.contains("websocket ktp_tcp"));
    assert!(script.contains("KELICLOUD_SMOKE_KTP_TCP=false"));
    assert!(script.contains("KELICLOUD_SMOKE_KTP_TCP=true"));
    assert!(script.contains("SMOKE_LOG_DIR="));
    assert!(script.contains("SMOKE_WORK_DIR="));
    assert!(script.contains("scripts/smoke-local-backend.sh"));
}

#[test]
fn ktp_local_backend_matrix_script_has_valid_bash_syntax_when_bash_is_available() {
    let Some(bash) = find_bash() else {
        eprintln!("bash not available; skipping syntax check");
        return;
    };

    let output = Command::new(bash)
        .arg("-n")
        .arg(script_path())
        .output()
        .expect("bash -n should run");

    assert!(
        output.status.success(),
        "bash -n failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn ktp_local_backend_matrix_script_dry_run_expands_websocket_and_ktp_tcp() {
    let Some(bash) = find_bash() else {
        eprintln!("bash not available; skipping dry-run check");
        return;
    };

    let output = Command::new(bash)
        .env("KELICLOUD_LOCAL_BACKEND_MATRIX_DRY_RUN", "1")
        .env(
            "KELICLOUD_LOCAL_BACKEND_MATRIX_CARRIERS",
            "websocket ktp_tcp",
        )
        .env(
            "KELICLOUD_LOCAL_BACKEND_MATRIX_LOG_DIR",
            "/tmp/kelicloud-matrix-logs",
        )
        .env(
            "KELICLOUD_LOCAL_BACKEND_MATRIX_WORK_DIR",
            "/tmp/kelicloud-matrix-work",
        )
        .arg(script_path())
        .output()
        .expect("matrix dry-run should run");

    assert!(
        output.status.success(),
        "matrix dry-run failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.matches("dry_run:").count(), 2);
    assert!(stdout.contains("carrier=websocket"));
    assert!(stdout.contains("KELICLOUD_SMOKE_KTP_TCP=false"));
    assert!(stdout.contains("SMOKE_LOG_DIR=/tmp/kelicloud-matrix-logs/websocket"));
    assert!(stdout.contains("SMOKE_WORK_DIR=/tmp/kelicloud-matrix-work/websocket"));
    assert!(stdout.contains("carrier=ktp_tcp"));
    assert!(stdout.contains("KELICLOUD_SMOKE_KTP_TCP=true"));
    assert!(stdout.contains("SMOKE_LOG_DIR=/tmp/kelicloud-matrix-logs/ktp_tcp"));
    assert!(stdout.contains("SMOKE_WORK_DIR=/tmp/kelicloud-matrix-work/ktp_tcp"));
}

#[test]
fn ktp_local_backend_matrix_script_rejects_unknown_carrier() {
    let Some(bash) = find_bash() else {
        eprintln!("bash not available; skipping invalid carrier check");
        return;
    };

    let output = Command::new(bash)
        .env("KELICLOUD_LOCAL_BACKEND_MATRIX_DRY_RUN", "1")
        .env("KELICLOUD_LOCAL_BACKEND_MATRIX_CARRIERS", "websocket udp")
        .arg(script_path())
        .output()
        .expect("matrix dry-run should run");

    assert!(
        !output.status.success(),
        "unknown carrier should fail:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown carrier: udp"));
}

fn script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("ktp-local-backend-matrix.sh")
}

fn find_bash() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(if cfg!(windows) { "bash.exe" } else { "bash" });
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    #[cfg(windows)]
    {
        for candidate in [
            r"C:\Program Files\Git\bin\bash.exe",
            r"C:\Program Files\Git\usr\bin\bash.exe",
        ] {
            let candidate = PathBuf::from(candidate);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}
