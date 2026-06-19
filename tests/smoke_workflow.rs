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
    assert!(workflow.contains("tunnel_echo_clients: \"1\""));
    assert!(workflow.contains("tunnel_echo_clients: \"4\""));
    assert!(workflow.contains("ktp_min_max_batch_frames: \"1\""));
    assert!(workflow.contains("ktp_min_max_batch_frames: \"2\""));
    assert!(workflow.contains("ktp_min_max_write_batch_frames: \"1\""));
    assert!(workflow.contains("ktp_min_max_write_batch_frames: \"2\""));
    assert!(workflow.contains("tunnel_echo_profile: fixed"));
    assert!(workflow.contains("tunnel_echo_profile: rdp-like"));
    assert!(workflow.contains("tunnel_echo_payload_bytes: \"0\""));
    assert!(workflow.contains("tunnel_echo_payload_bytes: \"8192\""));
    assert!(workflow.contains("KELICLOUD_SMOKE_KTP_TCP: ${{ matrix.ktp_tcp }}"));
    assert!(workflow.contains("KELICLOUD_TUNNEL_ECHO_ROUNDS: ${{ matrix.tunnel_echo_rounds }}"));
    assert!(workflow.contains("KELICLOUD_TUNNEL_ECHO_CLIENTS: ${{ matrix.tunnel_echo_clients }}"));
    assert!(workflow.contains("KELICLOUD_TUNNEL_ECHO_PROFILE: ${{ matrix.tunnel_echo_profile }}"));
    assert!(workflow
        .contains("KELICLOUD_TUNNEL_ECHO_PAYLOAD_BYTES: ${{ matrix.tunnel_echo_payload_bytes }}"));
    assert!(workflow
        .contains("KTP_LIVE_CANARY_MIN_MAX_BATCH_FRAMES: ${{ matrix.ktp_min_max_batch_frames }}"));
    assert!(workflow.contains(
        "KTP_LIVE_CANARY_MIN_MAX_WRITE_BATCH_FRAMES: ${{ matrix.ktp_min_max_write_batch_frames }}"
    ));
    assert!(workflow.contains("KOMARI_DB_NAME: komari_${{ matrix.data_plane }}"));
    assert!(workflow.contains("SMOKE_LOG_DIR: smoke-logs-${{ matrix.data_plane }}"));
    assert!(workflow.contains("bash scripts/smoke-local-backend.sh"));
    assert!(workflow.contains("smoke.status"));
    assert!(workflow.contains("actions/upload-artifact@v4"));
    assert!(workflow.contains("smoke-logs-${{ matrix.data_plane }}/*"));
    assert!(workflow.contains("kelicloud-agent-rs-local-backend-smoke-${{ matrix.data_plane }}"));
    assert!(workflow.contains("carrier-summary:"));
    assert!(workflow.contains("needs: linux"));
    assert!(workflow.contains("actions/download-artifact@v4"));
    assert!(workflow.contains("pattern: kelicloud-agent-rs-local-backend-smoke-*"));
    assert!(workflow.contains("carrier-matrix-artifacts"));
    assert!(workflow.contains("carrier-matrix-summary/matrix-summary.tsv"));
    assert!(workflow.contains("carrier-matrix-summary/matrix-summary.report.txt"));
    assert!(workflow.contains("markdown_value()"));
    assert!(workflow.contains("tunnel-echo.evidence.md"));
    assert!(workflow.contains("tunnel_evidence_file"));
    assert!(workflow.contains("tunnel_profile"));
    assert!(workflow.contains("tunnel_clients"));
    assert!(workflow.contains("tunnel_rounds"));
    assert!(workflow.contains("tunnel_total_payload_bytes"));
    assert!(workflow.contains("rtt_micros_p50"));
    assert!(workflow.contains("rtt_micros_p95"));
    assert!(workflow.contains("rtt_micros_p99"));
    assert!(workflow.contains("rtt_micros_max"));
    assert!(workflow.contains("rtt_client_p95_spread_micros"));
    assert!(workflow.contains("ktp-local-backend-matrix-summary"));
    assert!(workflow.contains("--require-pass"));
    assert!(workflow.contains("--require-ktp-aead"));
    assert!(workflow.contains("--require-ktp-tunnel-rtt"));
    assert!(workflow.contains("--require-ktp-rdp-like-rtt"));
    assert!(workflow.contains("--max-ktp-rdp-like-rtt-p95-micros"));
    assert!(workflow.contains("--max-ktp-rdp-like-client-p95-spread-micros"));
    assert!(workflow.contains("\"250000\""));
    assert!(workflow.contains("Local backend carrier matrix summary"));
    assert!(workflow.contains("kelicloud-agent-rs-local-backend-carrier-summary"));
}

fn local_backend_smoke_workflow_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".github")
        .join("workflows")
        .join("local-backend-smoke.yml")
}

#[test]
fn ktp_tunnel_matrix_workflow_runs_manual_local_backend_matrix() {
    let workflow = std::fs::read_to_string(ktp_tunnel_matrix_workflow_path()).unwrap();

    assert!(workflow.contains("name: KTP Tunnel Matrix"));
    assert!(workflow.contains("push:"));
    assert!(workflow.contains("branches:"));
    assert!(workflow.contains("- main"));
    assert!(workflow.contains("paths:"));
    assert!(workflow.contains("- .github/workflows/ktp-tunnel-matrix.yml"));
    assert!(workflow.contains("- src/ktp.rs"));
    assert!(workflow.contains("- src/config.rs"));
    assert!(workflow.contains("- src/ktp_transport.rs"));
    assert!(workflow.contains("- src/tunnel_control.rs"));
    assert!(workflow.contains("- src/tunnel_async_runtime.rs"));
    assert!(workflow.contains("- src/tunnel_data.rs"));
    assert!(workflow.contains("- src/tunnel_preflight.rs"));
    assert!(workflow.contains("- src/tunnel_runtime.rs"));
    assert!(workflow.contains("- tests/ktp.rs"));
    assert!(workflow.contains("- tests/config.rs"));
    assert!(workflow.contains("- tests/ktp_transport.rs"));
    assert!(workflow.contains("- tests/tunnel_control.rs"));
    assert!(workflow.contains("- tests/tunnel_async_runtime.rs"));
    assert!(workflow.contains("- tests/tunnel_data.rs"));
    assert!(workflow.contains("- tests/tunnel_preflight.rs"));
    assert!(workflow.contains("- tests/tunnel_runtime.rs"));
    assert!(workflow.contains("- scripts/ktp-local-backend-tunnel-matrix.sh"));
    assert!(workflow.contains("workflow_dispatch:"));
    assert!(workflow.contains("clients:"));
    assert!(workflow.contains("relay_batch_policies:"));
    assert!(workflow.contains("rounds:"));
    assert!(workflow.contains("payload_bytes:"));
    assert!(workflow.contains("min_max_batch_frames:"));
    assert!(workflow.contains("min_max_write_batch_frames:"));
    assert!(workflow.contains("adaptive_high_sessions:"));
    assert!(workflow.contains("adaptive_elevated_dwell_us:"));
    assert!(workflow.contains("adaptive_severe_dwell_us:"));
    assert!(workflow.contains("adaptive_elevated_cap:"));
    assert!(workflow.contains("adaptive_severe_cap:"));
    assert!(workflow.contains("client_timeout_seconds:"));
    assert!(workflow.contains("max_rtt_p95_micros:"));
    assert!(workflow.contains("max_client_p95_spread_micros:"));
    assert!(workflow.contains("min_throughput_mib_s:"));
    assert!(workflow.contains("min_echo_throughput_mib_s:"));
    assert!(workflow.contains("max_backend_session_limit_count:"));
    assert!(workflow.contains("max_backend_session_not_found_count:"));
    assert!(workflow.contains("summary_require_pass:"));
    assert!(workflow.contains("summary_fail_on_fixed_better:"));
    assert!(workflow.contains("default: \"1 2 4 8\""));
    assert!(workflow.contains("default: \"fixed adaptive\""));
    assert!(workflow.contains("default: \"8\""));
    assert!(workflow.contains("default: \"8192\""));
    assert!(workflow.contains("default: \"900\""));
    assert!(workflow.contains("default: \"50000\""));
    assert!(workflow.contains("default: \"250000\""));
    assert!(workflow.contains("default: \"16\""));
    assert!(workflow.contains("default: \"2\""));
    assert!(workflow.contains("default: \"true\""));
    assert!(workflow.contains("default: \"false\""));
    assert!(workflow.contains("default: \"\""));
    assert!(workflow.contains("mysql:8.4"));
    assert!(workflow.contains("KOMARI_DB_HOST: 127.0.0.1"));
    assert!(workflow.contains("actions/setup-go@v5"));
    assert!(workflow.contains("go-version: \"1.24.11\""));
    assert!(workflow.contains("actions/setup-node@v4"));
    assert!(workflow.contains("node-version: \"22\""));
    assert!(workflow.contains("rustup toolchain install stable"));
    assert!(workflow.contains("mysql-client"));
    assert!(workflow.contains("cancel-in-progress: true"));
    assert!(workflow
        .contains("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENTS: ${{ github.event_name == 'workflow_dispatch' && inputs.clients || '1 2' }}"));
    assert!(workflow.contains(
        "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_RELAY_BATCH_POLICIES: ${{ github.event_name == 'workflow_dispatch' && inputs.relay_batch_policies || 'fixed adaptive' }}"
    ));
    assert!(
        workflow.contains("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ROUNDS: ${{ github.event_name == 'workflow_dispatch' && inputs.rounds || '4' }}")
    );
    assert!(workflow.contains(
        "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_PAYLOAD_BYTES: ${{ github.event_name == 'workflow_dispatch' && inputs.payload_bytes || '4096' }}"
    ));
    assert!(workflow.contains(
        "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MIN_MAX_BATCH_FRAMES: ${{ github.event_name == 'workflow_dispatch' && inputs.min_max_batch_frames || '2' }}"
    ));
    assert!(workflow.contains(
        "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MIN_MAX_WRITE_BATCH_FRAMES: ${{ github.event_name == 'workflow_dispatch' && inputs.min_max_write_batch_frames || '2' }}"
    ));
    assert!(workflow.contains(
        "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ADAPTIVE_HIGH_SESSIONS: ${{ github.event_name == 'workflow_dispatch' && inputs.adaptive_high_sessions || '8' }}"
    ));
    assert!(workflow.contains(
        "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ADAPTIVE_ELEVATED_DWELL_US: ${{ github.event_name == 'workflow_dispatch' && inputs.adaptive_elevated_dwell_us || '50000' }}"
    ));
    assert!(workflow.contains(
        "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ADAPTIVE_SEVERE_DWELL_US: ${{ github.event_name == 'workflow_dispatch' && inputs.adaptive_severe_dwell_us || '250000' }}"
    ));
    assert!(workflow.contains(
        "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ADAPTIVE_ELEVATED_CAP: ${{ github.event_name == 'workflow_dispatch' && inputs.adaptive_elevated_cap || '16' }}"
    ));
    assert!(workflow.contains(
        "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ADAPTIVE_SEVERE_CAP: ${{ github.event_name == 'workflow_dispatch' && inputs.adaptive_severe_cap || '8' }}"
    ));
    assert!(workflow.contains(
        "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENT_TIMEOUT_SECONDS: ${{ github.event_name == 'workflow_dispatch' && inputs.client_timeout_seconds || '900' }}"
    ));
    assert!(workflow.contains(
        "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MAX_RTT_P95_MICROS: ${{ github.event_name == 'workflow_dispatch' && inputs.max_rtt_p95_micros || '' }}"
    ));
    assert!(workflow.contains(
        "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MAX_CLIENT_P95_SPREAD_MICROS: ${{ github.event_name == 'workflow_dispatch' && inputs.max_client_p95_spread_micros || '' }}"
    ));
    assert!(workflow.contains(
        "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MIN_THROUGHPUT_MIB_S: ${{ github.event_name == 'workflow_dispatch' && inputs.min_throughput_mib_s || '0.0001' }}"
    ));
    assert!(workflow.contains(
        "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MIN_ECHO_THROUGHPUT_MIB_S: ${{ github.event_name == 'workflow_dispatch' && inputs.min_echo_throughput_mib_s || '0.0001' }}"
    ));
    assert!(workflow.contains(
        "KTP_TUNNEL_MATRIX_MAX_BACKEND_SESSION_LIMIT_COUNT: ${{ github.event_name == 'workflow_dispatch' && inputs.max_backend_session_limit_count || '0' }}"
    ));
    assert!(workflow.contains(
        "KTP_TUNNEL_MATRIX_MAX_BACKEND_SESSION_NOT_FOUND_COUNT: ${{ github.event_name == 'workflow_dispatch' && inputs.max_backend_session_not_found_count || '0' }}"
    ));
    assert!(workflow.contains(
        "KTP_TUNNEL_MATRIX_SUMMARY_REQUIRE_PASS: ${{ github.event_name == 'workflow_dispatch' && inputs.summary_require_pass || 'true' }}"
    ));
    assert!(workflow.contains(
        "KTP_TUNNEL_MATRIX_SUMMARY_FAIL_ON_FIXED_BETTER: ${{ github.event_name == 'workflow_dispatch' && inputs.summary_fail_on_fixed_better || 'false' }}"
    ));
    assert!(workflow.contains("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_LOG_DIR: tunnel-matrix-logs"));
    assert!(workflow
        .contains("KTP_LOCAL_BACKEND_TUNNEL_MATRIX_WORK_DIR: /tmp/kelicloud-tunnel-matrix-work"));
    assert!(workflow.contains("bash scripts/ktp-local-backend-tunnel-matrix.sh"));
    assert!(workflow.contains("ktp-tunnel-matrix-summary"));
    assert!(workflow.contains("set -o pipefail"));
    assert!(workflow.contains("summary_args=()"));
    assert!(workflow.contains("summary_args+=(--require-pass)"));
    assert!(workflow.contains("summary_args+=(--fail-on-fixed-better)"));
    assert!(workflow
        .contains("summary_args+=(--max-rtt-p95-micros \"${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MAX_RTT_P95_MICROS}\")"));
    assert!(workflow.contains(
        "summary_args+=(--max-client-p95-spread-micros \"${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MAX_CLIENT_P95_SPREAD_MICROS}\")"
    ));
    assert!(workflow.contains(
        "summary_args+=(--min-throughput-mib-s \"${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MIN_THROUGHPUT_MIB_S}\")"
    ));
    assert!(workflow.contains(
        "summary_args+=(--min-echo-throughput-mib-s \"${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MIN_ECHO_THROUGHPUT_MIB_S}\")"
    ));
    assert!(workflow.contains(
        "summary_args+=(--max-backend-session-limit-count \"${KTP_TUNNEL_MATRIX_MAX_BACKEND_SESSION_LIMIT_COUNT}\")"
    ));
    assert!(workflow.contains(
        "summary_args+=(--max-backend-session-not-found-count \"${KTP_TUNNEL_MATRIX_MAX_BACKEND_SESSION_NOT_FOUND_COUNT}\")"
    ));
    assert!(workflow.contains(
        "summary_args+=(--expect-policies \"${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_RELAY_BATCH_POLICIES}\")"
    ));
    assert!(workflow.contains(
        "summary_args+=(--expect-clients \"${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENTS}\")"
    ));
    assert!(workflow.contains("cargo run --locked --bin ktp-tunnel-matrix-summary -- \"${summary_args[@]}\" \"${summary}\" | tee \"${report}\""));
    assert!(workflow.contains("matrix-summary.report.txt"));
    assert!(workflow.contains("KTP tunnel matrix summary"));
    assert!(workflow.contains("matrix-summary.tsv"));
    assert!(workflow.contains("GITHUB_STEP_SUMMARY"));
    assert!(workflow.contains("actions/upload-artifact@v4"));
    assert!(workflow.contains("kelicloud-agent-rs-ktp-tunnel-matrix"));
    assert!(workflow.contains("tunnel-matrix-logs/**/*"));
}

fn ktp_tunnel_matrix_workflow_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".github")
        .join("workflows")
        .join("ktp-tunnel-matrix.yml")
}

#[test]
fn ktp_microbench_matrix_workflow_publishes_codec_and_carrier_csv() {
    let workflow = std::fs::read_to_string(ktp_microbench_matrix_workflow_path()).unwrap();

    assert!(workflow.contains("name: KTP Microbench Matrix"));
    assert!(workflow.contains("push:"));
    assert!(workflow.contains("branches:"));
    assert!(workflow.contains("- main"));
    assert!(workflow.contains("paths:"));
    assert!(workflow.contains("- .github/workflows/ktp-microbench-matrix.yml"));
    assert!(workflow.contains("- src/ktp.rs"));
    assert!(workflow.contains("- src/ktp_transport.rs"));
    assert!(workflow.contains("- src/bin/ktp-codec-bench.rs"));
    assert!(workflow.contains("- src/bin/ktp-tunnel-bench.rs"));
    assert!(workflow.contains("- src/bin/ktp-carrier-matrix-summary.rs"));
    assert!(workflow.contains("- scripts/ktp-codec-matrix.sh"));
    assert!(workflow.contains("- scripts/ktp-carrier-matrix.sh"));
    assert!(workflow.contains("- tests/ktp_carrier_matrix_summary_cli.rs"));
    assert!(workflow.contains("workflow_dispatch:"));
    assert!(workflow.contains("codec_frames:"));
    assert!(workflow.contains("carrier_frames:"));
    assert!(workflow.contains("payload_bytes:"));
    assert!(workflow.contains("runs:"));
    assert!(workflow.contains("KTP_CODEC_MATRIX_CSV: microbench-logs/ktp-codec-matrix.csv"));
    assert!(workflow.contains("KTP_CARRIER_MATRIX_CSV: microbench-logs/ktp-carrier-matrix.csv"));
    assert!(workflow.contains(
        "cargo build --locked --release --bin ktp-codec-bench --bin ktp-tunnel-bench --bin ktp-carrier-matrix-summary"
    ));
    assert!(workflow.contains("bash scripts/ktp-codec-matrix.sh"));
    assert!(workflow.contains("bash scripts/ktp-carrier-matrix.sh"));
    assert!(workflow.contains("ktp-carrier-matrix-summary"));
    assert!(workflow.contains("--require-ktp-aead"));
    assert!(workflow.contains("--require-batch-reuse"));
    assert!(workflow.contains("--require-positive-throughput"));
    assert!(workflow.contains("ktp-carrier-matrix.report.txt"));
    assert!(workflow.contains("KTP microbench matrix summary"));
    assert!(workflow.contains("ktp-codec-matrix.csv"));
    assert!(workflow.contains("ktp-carrier-matrix.csv"));
    assert!(workflow.contains("GITHUB_STEP_SUMMARY"));
    assert!(workflow.contains("actions/upload-artifact@v4"));
    assert!(workflow.contains("kelicloud-agent-rs-ktp-microbench-matrix"));
    assert!(workflow.contains("microbench-logs/**/*"));
}

fn ktp_microbench_matrix_workflow_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".github")
        .join("workflows")
        .join("ktp-microbench-matrix.yml")
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
