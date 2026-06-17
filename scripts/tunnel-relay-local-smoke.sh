#!/usr/bin/env bash
set -euo pipefail

echo "== tunnel preflight checks =="
cargo test --test tunnel_preflight -- --nocapture

echo "== tunnel runtime happy path =="
cargo test --test tunnel_runtime tcp_runtime_two_agent_relay_simulation_forwards_echo -- --nocapture

echo "== async tunnel runtime performance gate =="
cargo test --test tunnel_async_runtime async_runtime_handles_100_concurrent_loopback_sessions -- --nocapture

echo "== tunnel runtime listener lifecycle =="
cargo test --test tunnel_runtime tcp_runtime_stops_listener_when_rule_is_removed -- --nocapture
cargo test --test tunnel_runtime tcp_runtime_restarts_listener_when_listen_port_changes -- --nocapture
cargo test --test tunnel_runtime tcp_runtime_removes_session_after_local_close -- --nocapture

echo "== tunnel runtime failure diagnostics =="
cargo test --test tunnel_runtime tcp_runtime_target_connect_failure_returns_stable_error_code_and_no_session -- --nocapture
echo "expected diagnostic: target_connect_failed"
cargo test --test tunnel_runtime tcp_runtime_start_failure_reports_listener_start_failed -- --nocapture
echo "expected diagnostic: listener_start_failed"
