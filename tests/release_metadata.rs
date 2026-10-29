#[test]
fn release_metadata_tracks_current_version() {
    let expected_tag = format!("v{}", env!("CARGO_PKG_VERSION"));

    let readme = std::fs::read_to_string("README.md").expect("README should be readable");
    assert!(
        readme.contains(&format!("--install-version {expected_tag}")),
        "README install-version examples should use {expected_tag}"
    );
}

#[test]
fn readme_documents_release_safety_gates() {
    let readme = std::fs::read_to_string("README.md").expect("README should be readable");

    assert!(readme.contains("bash scripts/tunnel-relay-local-smoke.sh"));
    assert!(readme.contains("KTP_SMOKE_POLICY_GATE=1"));
    assert!(readme.contains("actionlint"));
    assert!(readme.contains("KELICLOUD_RELEASE_CANARY=1"));
    assert!(readme.contains("real-host-canary.yml"));
    assert!(readme.contains("KELICLOUD_CANARY_TUNNEL_KTP_TCP_ADDRESS"));
    assert!(readme.contains("KELICLOUD_CANARY_TUNNEL_KTP_TCP_AUTH_VERSION"));
    assert!(readme.contains("KELICLOUD_CANARY_TUNNEL_KTP_RELAY_BATCH_POLICY"));
    assert!(readme.contains("actions/attest@v4"));
    assert!(readme.contains("gh attestation verify"));
    assert!(readme.contains("release-assets/SHA256SUMS"));
}
