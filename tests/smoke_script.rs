use std::path::PathBuf;

#[test]
fn smoke_script_documents_live_backend_checks() {
    let script = std::fs::read_to_string(smoke_script_path()).unwrap();

    assert!(script.contains("AGENT_ENDPOINT"));
    assert!(script.contains("AGENT_TOKEN"));
    assert!(script.contains("--mode once"));
    assert!(script.contains("--mode live"));
    assert!(script.contains("--duration"));
    assert!(script.contains("--expect-success-log"));
    assert!(script.contains("redact_token"));
    assert!(script.contains("agent loop: completed"));
    assert!(script.contains("timeout"));
}

fn smoke_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("smoke-live.sh")
}
