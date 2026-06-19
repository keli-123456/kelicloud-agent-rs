#!/usr/bin/env bash
set -euo pipefail

KTP_SMOKE_POLICY_GATE="${KTP_SMOKE_POLICY_GATE:-0}"
KTP_SMOKE_POLICY_CSV="${KTP_SMOKE_POLICY_CSV:-${TMPDIR:-/tmp}/ktp-relay-policy-smoke.csv}"
KTP_SMOKE_CARRIER_RUNS="${KTP_SMOKE_CARRIER_RUNS:-3}"
KTP_SMOKE_BATCH_READ_FRAMES="${KTP_SMOKE_BATCH_READ_FRAMES:-512}"
KTP_SMOKE_BATCH_READ_PAYLOAD_BYTES="${KTP_SMOKE_BATCH_READ_PAYLOAD_BYTES:-4096}"
KTP_SMOKE_CODEC_RUNS="${KTP_SMOKE_CODEC_RUNS:-3}"
KTP_SMOKE_CODEC_FRAMES="${KTP_SMOKE_CODEC_FRAMES:-4096}"
KTP_SMOKE_CODEC_PAYLOAD_BYTES="${KTP_SMOKE_CODEC_PAYLOAD_BYTES:-16384}"
KTP_SMOKE_CODEC_CHUNK_FRAMES="${KTP_SMOKE_CODEC_CHUNK_FRAMES:-64}"

echo "== tunnel preflight checks =="
cargo test --test tunnel_preflight -- --nocapture

echo "== tunnel runtime happy path =="
cargo test --test tunnel_runtime tcp_runtime_two_agent_relay_simulation_forwards_echo -- --nocapture

echo "== async tunnel runtime performance gate =="
cargo test --test tunnel_async_runtime async_runtime_handles_100_concurrent_loopback_sessions -- --nocapture

echo "== async tunnel runtime close-boundary gate =="
cargo test --test tunnel_async_runtime async_runtime_close_session_drops_queued_outbound_frames_for_that_session -- --nocapture

echo "== encrypted ktp tcp carrier performance gate =="
cargo test --test ktp_transport encrypted_tcp_stream_handles_100_concurrent_loopback_round_trips -- --nocapture
cargo test --test ktp_transport encrypted_tcp_frame_relay_handles_100_bidirectional_rounds -- --nocapture
echo "== ktp codec cursor performance gate =="
cargo run --bin ktp-codec-bench -- --mode stream --frames "${KTP_SMOKE_CODEC_FRAMES}" --payload-bytes "${KTP_SMOKE_CODEC_PAYLOAD_BYTES}" --chunk-frames "${KTP_SMOKE_CODEC_CHUNK_FRAMES}" --runs "${KTP_SMOKE_CODEC_RUNS}"
cargo run --bin ktp-codec-bench -- --mode crypto --frames "${KTP_SMOKE_CODEC_FRAMES}" --payload-bytes "${KTP_SMOKE_CODEC_PAYLOAD_BYTES}" --chunk-frames "${KTP_SMOKE_CODEC_CHUNK_FRAMES}" --runs "${KTP_SMOKE_CODEC_RUNS}"
cargo run --bin ktp-tunnel-bench -- --frames 4096 --payload-bytes 16384 --runs "${KTP_SMOKE_CARRIER_RUNS}"
cargo run --bin ktp-tunnel-bench -- --direction relay-to-client-batch-read --frames "${KTP_SMOKE_BATCH_READ_FRAMES}" --payload-bytes "${KTP_SMOKE_BATCH_READ_PAYLOAD_BYTES}"
cargo run --bin ktp-e2e-bench -- --latency --frames 16 --payload-bytes 1024
cargo run --bin ktp-e2e-bench -- --profile rdp-like --diagnostics --latency --relay-wait-timeout-us 100 --clients 2 --frames 16 --payload-bytes 8192

if [[ "${KTP_SMOKE_POLICY_GATE}" == "1" ]]; then
  echo "== ktp relay batch policy gate =="
  KTP_BATCH_MATRIX_BATCH_POLICIES="${KTP_BATCH_MATRIX_BATCH_POLICIES:-fixed adaptive}" \
    KTP_BATCH_MATRIX_CLIENTS="${KTP_BATCH_MATRIX_CLIENTS:-4}" \
    KTP_BATCH_MATRIX_BATCHES="${KTP_BATCH_MATRIX_BATCHES:-64}" \
    KTP_BATCH_MATRIX_RUNS="${KTP_BATCH_MATRIX_RUNS:-5}" \
    KTP_BATCH_MATRIX_FRAMES="${KTP_BATCH_MATRIX_FRAMES:-64}" \
    KTP_BATCH_MATRIX_PAYLOAD_BYTES="${KTP_BATCH_MATRIX_PAYLOAD_BYTES:-8192}" \
    KTP_BATCH_MATRIX_CSV="${KTP_SMOKE_POLICY_CSV}" \
    KTP_BATCH_MATRIX_FAIL_ON_FIXED_BETTER=1 \
    bash scripts/ktp-relay-batch-matrix.sh
else
  echo "== ktp relay batch policy gate skipped =="
  echo "set KTP_SMOKE_POLICY_GATE=1 to compare fixed/adaptive batch policies"
fi

echo "== tunnel runtime listener lifecycle =="
cargo test --test tunnel_runtime tcp_runtime_stops_listener_when_rule_is_removed -- --nocapture
cargo test --test tunnel_runtime tcp_runtime_restarts_listener_when_listen_port_changes -- --nocapture
cargo test --test tunnel_runtime tcp_runtime_removes_session_after_local_close -- --nocapture

echo "== tunnel runtime failure diagnostics =="
cargo test --test tunnel_runtime tcp_runtime_target_connect_failure_returns_stable_error_code_and_no_session -- --nocapture
echo "expected diagnostic: target_connect_failed"
cargo test --test tunnel_runtime tcp_runtime_start_failure_reports_listener_start_failed -- --nocapture
echo "expected diagnostic: listener_start_failed"
