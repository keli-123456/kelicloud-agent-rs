use std::path::PathBuf;
use std::process::Command;

#[test]
fn live_panel_control_smoke_script_triggers_exec_and_ping() {
    let script = std::fs::read_to_string(live_panel_control_smoke_script_path()).unwrap();

    for expected in [
        "Live panel control-plane smoke",
        "KELICLOUD_PANEL_COOKIE",
        "--endpoint URL",
        "--client UUID",
        "--ping-target HOST:PORT",
        "/api/admin/task/exec",
        "/api/admin/ping/add",
        "/api/admin/task/${EXEC_TASK_ID}/result/${CLIENT_UUID}",
        "smoke: task_result_uploaded",
        "smoke: ping_result_uploaded",
        "journalctl -u kelicloud-agent-rs",
        "kelicloud-agent-rs-live-exec-smoke",
    ] {
        assert!(script.contains(expected), "missing {expected}");
    }
}

#[test]
fn live_panel_control_smoke_script_has_valid_bash_syntax_when_bash_is_available() {
    let Some(bash) = find_bash() else {
        eprintln!("bash not available; skipping syntax check");
        return;
    };

    let output = Command::new(bash)
        .arg("-n")
        .arg(live_panel_control_smoke_script_path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "bash -n failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn live_panel_control_smoke_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("live-panel-control-smoke.sh")
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
