use std::process::Command;

#[test]
fn cli_version_prints_package_version_without_config() {
    let output = Command::new(env!("CARGO_BIN_EXE_kelicloud-agent-rs"))
        .arg("--version")
        .output()
        .expect("run kelicloud-agent-rs --version");

    assert!(
        output.status.success(),
        "status={:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("kelicloud-agent-rs {}", env!("CARGO_PKG_VERSION"))
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).trim().is_empty(),
        "version command should not need config: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
