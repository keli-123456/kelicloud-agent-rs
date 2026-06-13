use std::path::PathBuf;

#[test]
fn install_script_defines_systemd_service_and_config_paths() {
    let script = std::fs::read_to_string(install_script_path()).unwrap();

    assert!(script.contains("/usr/local/bin/kelicloud-agent-rs"));
    assert!(script.contains("/etc/kelicloud-agent-rs/config.env"));
    assert!(script.contains("kelicloud-agent-rs.service"));
    assert!(script.contains("render-service"));
    assert!(script.contains("render-env"));
}

#[cfg(unix)]
#[test]
fn render_service_outputs_systemd_unit() {
    let output = std::process::Command::new("bash")
        .arg(install_script_path())
        .arg("render-service")
        .arg("--bin")
        .arg("/tmp/kelicloud-agent-rs")
        .arg("--env")
        .arg("/tmp/config.env")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[Unit]"));
    assert!(stdout.contains("Description=kelicloud Agent RS"));
    assert!(stdout.contains("EnvironmentFile=/tmp/config.env"));
    assert!(stdout.contains("ExecStart=/tmp/kelicloud-agent-rs"));
    assert!(stdout.contains("Restart=always"));
    assert!(stdout.contains("WantedBy=multi-user.target"));
}

#[cfg(unix)]
#[test]
fn render_env_outputs_agent_environment() {
    let output = std::process::Command::new("bash")
        .arg(install_script_path())
        .arg("render-env")
        .arg("--endpoint")
        .arg("https://panel.example.com")
        .arg("--token")
        .arg("secret token")
        .arg("--disable-web-ssh")
        .arg("--interval")
        .arg("5")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AGENT_ENDPOINT='https://panel.example.com'"));
    assert!(stdout.contains("AGENT_TOKEN='secret token'"));
    assert!(stdout.contains("AGENT_DISABLE_WEB_SSH='true'"));
    assert!(stdout.contains("AGENT_INTERVAL='5'"));
}

fn install_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("install.sh")
}
