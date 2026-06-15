#!/usr/bin/env bash
set -euo pipefail

cargo test --test tunnel_runtime tcp_runtime_two_agent_relay_simulation_forwards_echo -- --nocapture
cargo test --test tunnel_runtime tcp_runtime_stops_listener_when_rule_is_removed -- --nocapture
cargo test --test tunnel_runtime tcp_runtime_restarts_listener_when_listen_port_changes -- --nocapture
cargo test --test tunnel_runtime tcp_runtime_removes_session_after_local_close -- --nocapture
