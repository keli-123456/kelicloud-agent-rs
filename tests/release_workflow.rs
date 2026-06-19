use std::path::PathBuf;

#[test]
fn release_workflow_builds_linux_assets_used_by_installer() {
    let workflow = std::fs::read_to_string(release_workflow_path()).unwrap();

    assert!(workflow.contains("name: Release"));
    assert!(workflow.contains("tags:"));
    assert!(workflow.contains("\"v*\""));
    assert!(workflow.contains("contents: write"));
    assert!(workflow.contains("actions: write"));
    assert!(workflow.contains("CC_x86_64_unknown_linux_musl: musl-gcc"));
    assert!(workflow.contains("apt-get install -y --no-install-recommends musl-tools"));
    assert!(workflow.contains("for attempt in 1 2 3; do"));
    assert!(workflow.contains("cargo install cross --locked"));
    assert!(workflow.contains("sleep $((attempt * 10))"));
    assert!(workflow.contains("Verify release smoke gates"));
    assert!(workflow.contains("bash scripts/tunnel-relay-local-smoke.sh"));
    assert!(workflow.contains("KTP_SMOKE_POLICY_GATE: \"1\""));
    assert!(workflow.contains("needs: verify"));
    assert!(workflow.contains("cross build --locked --release --target"));
    assert!(workflow.contains("sha256sum * > SHA256SUMS"));
    assert!(workflow.contains("sha256sum -c SHA256SUMS"));
    assert!(workflow.contains("release-assets/SHA256SUMS"));
    assert!(workflow.contains("softprops/action-gh-release@v2"));
    assert!(workflow.contains("uses: softprops/action-gh-release@v2\n        with:"));
    assert!(workflow.contains("dispatch-real-host-canary:"));
    assert!(workflow.contains("name: Dispatch real host canary"));
    assert!(workflow.contains("needs: publish"));
    assert!(workflow.contains("vars.KELICLOUD_RELEASE_CANARY == '1'"));
    assert!(workflow.contains("gh workflow run real-host-canary.yml"));
    assert!(workflow.contains("--ref \"${{ github.event.repository.default_branch }}\""));
    assert!(workflow.contains("-f install_version=\"${GITHUB_REF_NAME}\""));

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
