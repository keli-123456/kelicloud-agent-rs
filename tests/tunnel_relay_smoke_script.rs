use std::process::Command;

#[test]
fn tunnel_relay_smoke_script_runs_runtime_relay_test() {
    let script = std::fs::read_to_string("scripts/tunnel-relay-local-smoke.sh")
        .expect("smoke script should be readable");
    assert!(script.contains("tcp_runtime_two_agent_relay_simulation_forwards_echo"));
    assert!(script.contains("cargo test --test tunnel_runtime"));
    assert!(script.contains("cargo test --test tunnel_async_runtime"));
    assert!(script.contains("async_runtime_handles_100_concurrent_loopback_sessions"));
    assert!(script.contains("cargo test --test ktp_transport"));
    assert!(script.contains("encrypted_tcp_stream_handles_100_concurrent_loopback_round_trips"));
    assert!(script.contains("encrypted_tcp_frame_relay_handles_100_bidirectional_rounds"));
}

#[test]
fn tunnel_relay_smoke_script_covers_preflight_and_failure_diagnostics() {
    let script = std::fs::read_to_string("scripts/tunnel-relay-local-smoke.sh")
        .expect("smoke script should be readable");
    assert!(script.contains("cargo test --test tunnel_preflight"));
    assert!(script
        .contains("tcp_runtime_target_connect_failure_returns_stable_error_code_and_no_session"));
    assert!(script.contains("tcp_runtime_start_failure_reports_listener_start_failed"));
    assert!(script.contains("target_connect_failed"));
    assert!(script.contains("listener_start_failed"));
}

#[test]
fn tunnel_relay_smoke_script_has_valid_bash_syntax_when_bash_is_available() {
    if Command::new("bash").arg("--version").output().is_err() {
        return;
    }
    let status = Command::new("bash")
        .args(["-n", "scripts/tunnel-relay-local-smoke.sh"])
        .status()
        .expect("bash -n should run");
    assert!(status.success());
}
