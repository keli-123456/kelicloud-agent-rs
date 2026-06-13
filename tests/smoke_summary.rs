use kelicloud_agent_rs::smoke_summary::{
    smoke_event_line, summarize_log_text, SmokeEvidenceStatus, CONTROL_CN_CONNECTIVITY,
    CONTROL_EXEC_RESULT, CONTROL_PING_RESULT, CONTROL_TERMINAL_SESSION, CORE_AGENT_STARTED,
    CORE_LOOP_COMPLETED, CORE_NO_ERRORS,
};

#[test]
fn smoke_event_line_formats_sanitized_key_value_fields() {
    assert_eq!(
        smoke_event_line("ping_result_uploaded", &[("task_id", "7"), ("value", "25")]),
        "smoke: ping_result_uploaded task_id=7 value=25"
    );
    assert_eq!(
        smoke_event_line("terminal_session_started", &[("request id", "term\n1")]),
        "smoke: terminal_session_started request_id=term 1"
    );
}

#[test]
fn smoke_summary_marks_observed_core_and_control_plane_evidence() {
    let log = r#"
Smoke mode: live
kelicloud-agent-rs prototype
endpoint: https://panel.example.com
smoke: basic_info_uploaded
smoke: report_websocket_connected
smoke: report_sent
smoke: ping_result_uploaded task_id=7 value=25
smoke: task_result_uploaded task_id=task-1 exit_code=0
smoke: terminal_session_started request_id=term-1
smoke: cn_connectivity_config_received targets=2
agent loop: completed
"#;

    let summary = summarize_log_text("live.log", log);

    assert_eq!(
        summary.evidence_status(CORE_AGENT_STARTED),
        Some(SmokeEvidenceStatus::Pass)
    );
    assert_eq!(
        summary.evidence_status(CORE_LOOP_COMPLETED),
        Some(SmokeEvidenceStatus::Pass)
    );
    assert_eq!(
        summary.evidence_status(CORE_NO_ERRORS),
        Some(SmokeEvidenceStatus::Pass)
    );
    assert_eq!(
        summary.evidence_status(CONTROL_PING_RESULT),
        Some(SmokeEvidenceStatus::Pass)
    );
    assert_eq!(
        summary.evidence_status(CONTROL_EXEC_RESULT),
        Some(SmokeEvidenceStatus::Pass)
    );
    assert_eq!(
        summary.evidence_status(CONTROL_TERMINAL_SESSION),
        Some(SmokeEvidenceStatus::Pass)
    );
    assert_eq!(
        summary.evidence_status(CONTROL_CN_CONNECTIVITY),
        Some(SmokeEvidenceStatus::Pass)
    );
    assert!(summary.is_pass());
    assert!(summary.failed_or_missing_evidence().is_empty());

    let markdown = summary.to_markdown();
    assert!(markdown.contains("# kelicloud-agent-rs Smoke Summary"));
    assert!(markdown.contains("| Ping result upload | pass | observed |"));
    assert!(markdown.contains("| Exec task result upload | pass | observed |"));
    assert!(markdown.contains("No missing live control-plane evidence."));
}

#[test]
fn smoke_summary_lists_missing_control_plane_checks_as_compatibility_gaps() {
    let log = r#"
Smoke mode: once
kelicloud-agent-rs prototype
endpoint: https://panel.example.com
agent loop: completed
"#;

    let summary = summarize_log_text("once.log", log);

    assert_eq!(
        summary.evidence_status(CORE_AGENT_STARTED),
        Some(SmokeEvidenceStatus::Pass)
    );
    assert_eq!(
        summary.evidence_status(CORE_LOOP_COMPLETED),
        Some(SmokeEvidenceStatus::Pass)
    );
    assert_eq!(
        summary.evidence_status(CONTROL_PING_RESULT),
        Some(SmokeEvidenceStatus::Missing)
    );
    assert!(!summary.is_pass());
    assert_eq!(
        summary.failed_or_missing_evidence()[0].label,
        "Ping result upload"
    );

    let markdown = summary.to_markdown();
    assert!(markdown.contains("| Ping result upload | missing | not observed |"));
    assert!(markdown.contains("- Ping task was not observed"));
    assert!(markdown.contains("- Exec task result upload was not observed"));
    assert!(markdown.contains("- Terminal session was not observed"));
    assert!(markdown.contains("- CN connectivity config was not observed"));
}

#[test]
fn smoke_summary_surfaces_runtime_errors_as_failures() {
    let log = r#"
kelicloud-agent-rs prototype
runtime error: request failed: websocket closed
"#;

    let summary = summarize_log_text("failed.log", log);

    assert_eq!(
        summary.evidence_status(CORE_NO_ERRORS),
        Some(SmokeEvidenceStatus::Fail)
    );
    assert!(!summary.is_pass());
    let markdown = summary.to_markdown();
    assert!(markdown.contains(
        "| No fatal agent errors | fail | runtime error: request failed: websocket closed |"
    ));
}
