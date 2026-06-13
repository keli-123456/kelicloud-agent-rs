use std::path::PathBuf;
use std::process::Command;

#[test]
fn local_backend_smoke_script_orchestrates_real_backend_controls() {
    let script = std::fs::read_to_string(local_backend_smoke_script_path()).unwrap();

    assert!(script.contains("KELICLOUD_BACKEND_REPO"));
    assert!(script.contains("KELICLOUD_BACKEND_REF"));
    assert!(script.contains("scripts/prepare-frontend.sh"));
    assert!(script.contains("/api/login"));
    assert!(script.contains("/api/admin/client/add"));
    assert!(script.contains("/api/admin/task/exec"));
    assert!(script.contains("/api/admin/ping/add"));
    assert!(script.contains("/api/admin/settings/system"));
    assert!(script.contains("admin-terminal-smoke"));
    assert!(script.contains("smoke-summary --require-pass"));
    assert!(script.contains("wait_for_log"));
    assert!(script.contains("smoke: ping_result_uploaded"));
    assert!(script.contains("smoke: task_result_uploaded"));
    assert!(script.contains("smoke: terminal_session_started"));
    assert!(script.contains("smoke: cn_connectivity_config_received"));
    assert!(script.contains("trap on_error ERR"));
    assert!(script.contains("CURRENT_STAGE"));
    assert!(script.contains("::error title=Local backend smoke::"));
    assert!(script.contains("sys.argv[1]"));
    assert!(!script.contains("os.environ[\"ADMIN_USERNAME\"]"));
}

#[test]
fn local_backend_smoke_script_has_valid_bash_syntax_when_bash_is_available() {
    let Some(bash) = find_bash() else {
        eprintln!("bash not available; skipping syntax check");
        return;
    };
    let output = Command::new(bash)
        .arg("-n")
        .arg(local_backend_smoke_script_path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "bash -n failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn local_backend_smoke_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("smoke-local-backend.sh")
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
