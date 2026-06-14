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

#[test]
fn install_script_exposes_panel_compatible_linux_flags() {
    let script = std::fs::read_to_string(install_script_path()).unwrap();

    for expected in [
        "-e, --endpoint URL",
        "-t, --token TOKEN",
        "--auto-discovery KEY",
        "--install-version VERSION",
        "--install-ghproxy URL",
        "--install-dir PATH",
        "--ignore-unsafe-cert",
        "--memory-include-cache",
        "--include-nics CSV",
        "--exclude-nics CSV",
        "--include-mountpoint LIST",
        "--month-rotate DAY",
        "AGENT_AUTO_DISCOVERY_KEY",
        "AGENT_MEMORY_INCLUDE_CACHE",
        "AGENT_INCLUDE_NICS",
        "AGENT_EXCLUDE_NICS",
        "AGENT_INCLUDE_MOUNTPOINTS",
        "AGENT_MONTH_ROTATE",
    ] {
        assert!(script.contains(expected), "missing {expected}");
    }
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

#[cfg(unix)]
#[test]
fn render_env_accepts_panel_compatible_auto_discovery_arguments() {
    let output = std::process::Command::new("bash")
        .arg(install_script_path())
        .arg("-e")
        .arg("https://panel.example.com")
        .arg("--auto-discovery")
        .arg("discovery-key")
        .arg("--disable-web-ssh")
        .arg("--ignore-unsafe-cert")
        .arg("--memory-include-cache")
        .arg("--include-nics")
        .arg("eth0,eth1")
        .arg("--exclude-nics")
        .arg("lo")
        .arg("--include-mountpoint")
        .arg("/;/data")
        .arg("--month-rotate")
        .arg("3")
        .arg("render-env")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AGENT_ENDPOINT='https://panel.example.com'"));
    assert!(stdout.contains("AGENT_AUTO_DISCOVERY_KEY='discovery-key'"));
    assert!(stdout.contains("AGENT_DISABLE_WEB_SSH='true'"));
    assert!(stdout.contains("AGENT_INSECURE='true'"));
    assert!(stdout.contains("AGENT_MEMORY_INCLUDE_CACHE='true'"));
    assert!(stdout.contains("AGENT_INCLUDE_NICS='eth0,eth1'"));
    assert!(stdout.contains("AGENT_EXCLUDE_NICS='lo'"));
    assert!(stdout.contains("AGENT_INCLUDE_MOUNTPOINTS='/;/data'"));
    assert!(stdout.contains("AGENT_MONTH_ROTATE='3'"));
    assert!(!stdout.contains("AGENT_TOKEN="));
}

#[test]
fn panel_style_command_defaults_to_install_when_command_is_omitted() {
    let script = std::fs::read_to_string(install_script_path()).unwrap();

    assert!(script.contains("if [[ -z \"$COMMAND\" ]]; then"));
    assert!(script.contains("COMMAND=\"install\""));
}

#[test]
fn install_script_stops_existing_service_before_replacing_binary() {
    let script = std::fs::read_to_string(install_script_path()).unwrap();

    assert!(script.contains("stop_existing_service_for_upgrade"));
    assert!(script.contains("systemctl stop \"${SERVICE_NAME}.service\""));
    assert!(script.contains("stop_existing_service_for_upgrade\n    install_binary"));
}

#[cfg(unix)]
#[test]
fn render_env_maps_install_version_and_github_proxy_aliases() {
    let output = std::process::Command::new("bash")
        .arg(install_script_path())
        .arg("render-env")
        .arg("-e")
        .arg("https://panel.example.com")
        .arg("-t")
        .arg("client-token")
        .arg("--install-version")
        .arg("v0.2.0")
        .arg("--install-ghproxy")
        .arg("https://ghfast.top")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AGENT_ENDPOINT='https://panel.example.com'"));
    assert!(stdout.contains("AGENT_TOKEN='client-token'"));
}

fn install_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("install.sh")
}
