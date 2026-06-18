use std::path::PathBuf;

#[test]
fn smoke_workflow_runs_live_script_on_manual_dispatch() {
    let workflow = std::fs::read_to_string(smoke_workflow_path()).unwrap();

    assert!(workflow.contains("name: Smoke"));
    assert!(workflow.contains("workflow_dispatch:"));
    assert!(workflow.contains("KELICLOUD_SMOKE_ENDPOINT"));
    assert!(workflow.contains("KELICLOUD_SMOKE_TOKEN"));
    assert!(workflow.contains("KELICLOUD_SMOKE_AUTO_DISCOVERY_KEY"));
    assert!(workflow.contains("KELICLOUD_SMOKE_CF_ACCESS_CLIENT_ID"));
    assert!(workflow.contains("KELICLOUD_SMOKE_CF_ACCESS_CLIENT_SECRET"));
    assert!(workflow.contains("::add-mask::$AGENT_TOKEN"));
    assert!(workflow.contains("::add-mask::$AGENT_AUTO_DISCOVERY_KEY"));
    assert!(workflow.contains("::add-mask::$AGENT_CF_ACCESS_CLIENT_SECRET"));
    assert!(workflow.contains("custom_dns:"));
    assert!(workflow.contains("insecure:"));
    assert!(workflow.contains("require_summary_pass:"));
    assert!(workflow.contains("scripts/smoke-live.sh"));
    assert!(workflow.contains("--mode \"${SMOKE_MODE}\""));
    assert!(workflow.contains("--duration \"${SMOKE_DURATION}\""));
    assert!(workflow.contains("--custom-dns \"${SMOKE_CUSTOM_DNS}\""));
    assert!(workflow.contains("--insecure"));
    assert!(workflow.contains("--require-summary-pass"));
    assert!(workflow.contains("actions/upload-artifact@v4"));
    assert!(workflow.contains("smoke-logs/*"));
}

fn smoke_workflow_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".github")
        .join("workflows")
        .join("smoke.yml")
}

#[test]
fn local_backend_smoke_workflow_runs_full_linux_control_plane() {
    let workflow = std::fs::read_to_string(local_backend_smoke_workflow_path()).unwrap();

    assert!(workflow.contains("name: Local Backend Smoke"));
    assert!(workflow.contains("push:"));
    assert!(workflow.contains("workflow_dispatch:"));
    assert!(workflow.contains("mysql:8.4"));
    assert!(workflow.contains("KOMARI_DB_HOST: 127.0.0.1"));
    assert!(workflow.contains("actions/setup-go@v5"));
    assert!(workflow.contains("actions/setup-node@v4"));
    assert!(workflow.contains("rustup toolchain install stable"));
    assert!(workflow.contains("strategy:"));
    assert!(workflow.contains("fail-fast: false"));
    assert!(workflow.contains("data_plane: websocket"));
    assert!(workflow.contains("data_plane: ktp_tcp"));
    assert!(workflow.contains("ktp_tcp: \"false\""));
    assert!(workflow.contains("ktp_tcp: \"true\""));
    assert!(workflow.contains("tunnel_echo_rounds: \"1\""));
    assert!(workflow.contains("tunnel_echo_rounds: \"8\""));
    assert!(workflow.contains("ktp_min_max_batch_frames: \"1\""));
    assert!(workflow.contains("ktp_min_max_batch_frames: \"2\""));
    assert!(workflow.contains("tunnel_echo_profile: fixed"));
    assert!(workflow.contains("tunnel_echo_profile: rdp-like"));
    assert!(workflow.contains("tunnel_echo_payload_bytes: \"0\""));
    assert!(workflow.contains("tunnel_echo_payload_bytes: \"8192\""));
    assert!(workflow.contains("KELICLOUD_SMOKE_KTP_TCP: ${{ matrix.ktp_tcp }}"));
    assert!(
        workflow
            .contains("KELICLOUD_TUNNEL_ECHO_ROUNDS: ${{ matrix.tunnel_echo_rounds }}")
    );
    assert!(
        workflow.contains("KELICLOUD_TUNNEL_ECHO_PROFILE: ${{ matrix.tunnel_echo_profile }}")
    );
    assert!(
        workflow.contains(
            "KELICLOUD_TUNNEL_ECHO_PAYLOAD_BYTES: ${{ matrix.tunnel_echo_payload_bytes }}"
        )
    );
    assert!(
        workflow.contains(
            "KTP_LIVE_CANARY_MIN_MAX_BATCH_FRAMES: ${{ matrix.ktp_min_max_batch_frames }}"
        )
    );
    assert!(workflow.contains("KOMARI_DB_NAME: komari_${{ matrix.data_plane }}"));
    assert!(workflow.contains("SMOKE_LOG_DIR: smoke-logs-${{ matrix.data_plane }}"));
    assert!(workflow.contains("bash scripts/smoke-local-backend.sh"));
    assert!(workflow.contains("actions/upload-artifact@v4"));
    assert!(workflow.contains("smoke-logs-${{ matrix.data_plane }}/*"));
    assert!(workflow.contains("kelicloud-agent-rs-local-backend-smoke-${{ matrix.data_plane }}"));
}

fn local_backend_smoke_workflow_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".github")
        .join("workflows")
        .join("local-backend-smoke.yml")
}

#[test]
fn real_host_canary_workflow_runs_on_self_hosted_runner() {
    let workflow = std::fs::read_to_string(real_host_canary_workflow_path()).unwrap();

    assert!(workflow.contains("name: Real Host Canary"));
    assert!(workflow.contains("workflow_dispatch:"));
    assert!(workflow.contains("runner_labels:"));
    assert!(workflow.contains("[\"self-hosted\",\"Linux\",\"kelicloud-canary\"]"));
    assert!(workflow.contains("runs-on: ${{ fromJSON(inputs.runner_labels) }}"));
    assert!(workflow.contains("KELICLOUD_CANARY_ENDPOINT"));
    assert!(workflow.contains("KELICLOUD_CANARY_AUTO_DISCOVERY_KEY"));
    assert!(workflow.contains("KELICLOUD_CANARY_ROLLBACK_COMMAND"));
    assert!(workflow.contains("require_rollback:"));
    assert!(workflow.contains("keep_installed:"));
    assert!(workflow.contains("control_plane:"));
    assert!(workflow.contains("derive_auto_discovery_from_old_service:"));
    assert!(workflow.contains("ping_target:"));
    assert!(workflow.contains("old_service_name:"));
    assert!(workflow.contains("rollback_service_name:"));
    assert!(workflow.contains("::add-mask::${CANARY_AUTO_DISCOVERY_KEY}"));
    assert!(workflow.contains("::add-mask::${CANARY_ROLLBACK_COMMAND}"));
    assert!(workflow.contains("::add-mask::${CANARY_PANEL_COOKIE}"));
    assert!(workflow.contains("::add-mask::${CANARY_PANEL_PASSWORD}"));
    assert!(workflow.contains("KELICLOUD_PANEL_COOKIE"));
    assert!(workflow.contains("KELICLOUD_PANEL_USERNAME"));
    assert!(workflow.contains("KELICLOUD_PANEL_PASSWORD"));
    assert!(workflow.contains("CANARY_DERIVE_AUTO_DISCOVERY_FROM_OLD_SERVICE"));
    assert!(workflow.contains("Derive auto-discovery from old service"));
    assert!(workflow.contains("systemctl cat \"${CANARY_OLD_SERVICE_NAME}.service\""));
    assert!(workflow.contains("--auto-discovery[=[:space:]]"));
    assert!(workflow.contains("CANARY_AUTO_DISCOVERY_KEY="));
    assert!(workflow.contains("GITHUB_ENV"));
    assert!(workflow.contains("sudo bash scripts/canary-install.sh"));
    assert!(workflow.contains("sudo --preserve-env=KELICLOUD_PANEL_COOKIE,KELICLOUD_PANEL_USERNAME,KELICLOUD_PANEL_PASSWORD bash scripts/real-host-control-canary.sh"));
    assert!(workflow.contains("--workdir canary-logs"));
    assert!(workflow.contains("--old-service \"${CANARY_OLD_SERVICE_NAME}\""));
    assert!(workflow.contains("--ping-target \"${CANARY_PANEL_PING_TARGET}\""));
    assert!(workflow.contains("--evidence-file canary-logs/real-host-canary.evidence.md"));
    assert!(workflow.contains("--rollback-command \"${CANARY_ROLLBACK_COMMAND}\""));
    assert!(workflow.contains("canary-logs/real-host-canary.log"));
    assert!(workflow.contains("canary-logs/real-host-control-canary.log"));
    assert!(workflow.contains("real-host-canary.evidence.md"));
    assert!(workflow.contains("real-host-control-canary.evidence.md"));
    assert!(workflow.contains("actions/upload-artifact@v4"));
    assert!(workflow.contains("kelicloud-agent-rs-real-host-canary"));
}

fn real_host_canary_workflow_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".github")
        .join("workflows")
        .join("real-host-canary.yml")
}
