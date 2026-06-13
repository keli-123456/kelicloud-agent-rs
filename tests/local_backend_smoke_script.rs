use std::path::PathBuf;

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
}

fn local_backend_smoke_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("smoke-local-backend.sh")
}
