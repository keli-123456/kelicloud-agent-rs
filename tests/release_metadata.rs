const EXPECTED_RELEASE_VERSION: &str = "0.2.4";
const EXPECTED_RELEASE_TAG: &str = "v0.2.4";

#[test]
fn release_metadata_tracks_current_version() {
    assert_eq!(env!("CARGO_PKG_VERSION"), EXPECTED_RELEASE_VERSION);

    let readme = std::fs::read_to_string("README.md").expect("README should be readable");
    assert!(
        readme.contains(&format!("--install-version {EXPECTED_RELEASE_TAG}")),
        "README install-version examples should use {EXPECTED_RELEASE_TAG}"
    );
}
