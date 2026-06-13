use std::path::PathBuf;

#[test]
fn release_workflow_builds_linux_assets_used_by_installer() {
    let workflow = std::fs::read_to_string(release_workflow_path()).unwrap();

    assert!(workflow.contains("name: Release"));
    assert!(workflow.contains("tags:"));
    assert!(workflow.contains("\"v*\""));
    assert!(workflow.contains("contents: write"));
    assert!(workflow.contains("cross build --locked --release --target"));
    assert!(workflow.contains("softprops/action-gh-release@v2"));

    for (target, asset) in [
        (
            "x86_64-unknown-linux-musl",
            "kelicloud-agent-rs-linux-amd64",
        ),
        (
            "aarch64-unknown-linux-musl",
            "kelicloud-agent-rs-linux-arm64",
        ),
        (
            "armv7-unknown-linux-musleabihf",
            "kelicloud-agent-rs-linux-armv7",
        ),
    ] {
        assert!(workflow.contains(target), "missing target {target}");
        assert!(workflow.contains(asset), "missing asset {asset}");
    }
}

fn release_workflow_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".github")
        .join("workflows")
        .join("release.yml")
}
