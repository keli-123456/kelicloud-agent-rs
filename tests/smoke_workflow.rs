use std::path::PathBuf;

#[test]
fn smoke_workflow_runs_live_script_on_manual_dispatch() {
    let workflow = std::fs::read_to_string(smoke_workflow_path()).unwrap();

    assert!(workflow.contains("name: Smoke"));
    assert!(workflow.contains("workflow_dispatch:"));
    assert!(workflow.contains("KELICLOUD_SMOKE_ENDPOINT"));
    assert!(workflow.contains("KELICLOUD_SMOKE_TOKEN"));
    assert!(workflow.contains("::add-mask::$AGENT_TOKEN"));
    assert!(workflow.contains("scripts/smoke-live.sh"));
    assert!(workflow.contains("--mode \"${SMOKE_MODE}\""));
    assert!(workflow.contains("--duration \"${SMOKE_DURATION}\""));
    assert!(workflow.contains("actions/upload-artifact@v4"));
    assert!(workflow.contains("smoke-logs/*.log"));
}

fn smoke_workflow_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".github")
        .join("workflows")
        .join("smoke.yml")
}
