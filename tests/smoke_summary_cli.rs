use std::fs;
use std::process::Command;

#[test]
fn smoke_summary_cli_require_pass_fails_when_evidence_is_missing() {
    let log_path = unique_temp_log("missing");
    fs::write(
        &log_path,
        "kelicloud-agent-rs prototype\nagent loop: completed\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_smoke-summary"))
        .arg("--require-pass")
        .arg(&log_path)
        .output()
        .unwrap();
    let _ = fs::remove_file(&log_path);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("smoke summary did not pass"));
    assert!(stderr.contains("Ping result upload"));
}

#[test]
fn smoke_summary_cli_require_pass_succeeds_when_all_evidence_is_observed() {
    let log_path = unique_temp_log("pass");
    fs::write(
        &log_path,
        r#"
kelicloud-agent-rs prototype
smoke: basic_info_uploaded
smoke: report_websocket_connected
smoke: report_sent
smoke: ping_result_uploaded task_id=7 value=25
smoke: task_result_uploaded task_id=task-1 exit_code=0
smoke: terminal_session_started request_id=term-1
smoke: cn_connectivity_config_received enabled=true
agent loop: completed
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_smoke-summary"))
        .arg("--require-pass")
        .arg(&log_path)
        .output()
        .unwrap();
    let _ = fs::remove_file(&log_path);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No missing live control-plane evidence."));
}

fn unique_temp_log(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "kelicloud-agent-rs-smoke-summary-{name}-{}-{}.log",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ))
}
