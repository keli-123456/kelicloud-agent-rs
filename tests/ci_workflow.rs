use std::path::PathBuf;

#[test]
fn ci_workflow_lints_github_actions_workflows() {
    let workflow = std::fs::read_to_string(ci_workflow_path()).unwrap();

    assert!(workflow.contains("name: CI"));
    assert!(workflow.contains("Check GitHub Actions workflows"));
    assert!(workflow.contains("uses: rhysd/actionlint@v1"));
}

fn ci_workflow_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".github")
        .join("workflows")
        .join("ci.yml")
}
