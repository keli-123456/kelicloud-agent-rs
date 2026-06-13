# Agent RS Online Loop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first Rust agent milestone that can upload basic info, connect the report WebSocket, send compatible reports, and parse backend control messages.

**Architecture:** Keep protocol models, report payloads, transport interfaces, and runtime orchestration separate. Runtime code depends on traits so tests can exercise behavior without live backend access.

**Tech Stack:** Rust 2021, `serde`, `serde_json`, `tokio`, `reqwest`, `tokio-tungstenite`, `futures-util`, `url`, and `sysinfo` for minimal host data.

---

### Task 1: Extend Core Configuration

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/config.rs`
- Test: `tests/config.rs`

- [ ] Write failing tests for interval, retry, reconnect interval, info report interval, and Cloudflare Access headers.
- [ ] Run `cargo test config` and verify the new tests fail because the fields and flags do not exist.
- [ ] Add fields to `AgentConfig`: `interval_seconds`, `max_retries`, `reconnect_interval_seconds`, `info_report_interval_minutes`, `cf_access_client_id`, and `cf_access_client_secret`.
- [ ] Parse CLI flags and environment variables: `AGENT_INTERVAL`, `AGENT_MAX_RETRIES`, `AGENT_RECONNECT_INTERVAL`, `AGENT_INFO_REPORT_INTERVAL`, `AGENT_CF_ACCESS_CLIENT_ID`, and `AGENT_CF_ACCESS_CLIENT_SECRET`.
- [ ] Run `cargo test config` and verify it passes.

### Task 2: Add Report and Basic Info Models

**Files:**
- Create: `src/report.rs`
- Modify: `src/lib.rs`
- Test: `tests/report.rs`

- [ ] Write failing tests for serializing a report JSON shape accepted by backend `common.Report`.
- [ ] Write failing tests for serializing basic info fields accepted by `/api/clients/uploadBasicInfo`.
- [ ] Run `cargo test report` and verify the tests fail because the module is missing.
- [ ] Implement `Report`, `CpuReport`, `MemoryReport`, `LoadReport`, `DiskReport`, `NetworkReport`, `ConnectionsReport`, `BasicInfo`, and `ReportGenerator`.
- [ ] Add a minimal `StaticReportGenerator` that returns valid placeholder data while real OS probing is built later.
- [ ] Run `cargo test report` and verify it passes.

### Task 3: Parse Backend Control Messages

**Files:**
- Modify: `src/protocol.rs`
- Test: `tests/protocol.rs`

- [ ] Write failing tests for parsing `cn_connectivity_probe_config`, `terminal`, `exec`, and `ping` messages.
- [ ] Run `cargo test protocol` and verify the new tests fail because parsing does not exist.
- [ ] Add `BackendMessage` and `parse_backend_message`.
- [ ] Preserve unknown JSON messages as `BackendMessage::Unknown`.
- [ ] Run `cargo test protocol` and verify it passes.

### Task 4: Add Transport Interfaces

**Files:**
- Create: `src/transport.rs`
- Modify: `src/lib.rs`
- Test: `tests/transport.rs`

- [ ] Write failing tests for building the basic info URL and Cloudflare Access headers.
- [ ] Run `cargo test transport` and verify the tests fail because the module is missing.
- [ ] Implement `build_basic_info_url`, `access_headers`, and transport traits for HTTP and WebSocket.
- [ ] Add concrete `ReqwestHttpTransport` and a placeholder WebSocket constructor boundary.
- [ ] Run `cargo test transport` and verify it passes.

### Task 5: Runtime Online Loop Orchestration

**Files:**
- Modify: `src/runtime.rs`
- Modify: `src/main.rs`
- Test: `tests/runtime.rs`

- [ ] Write failing tests using fake transports: startup uploads basic info before opening the WebSocket.
- [ ] Write failing tests: runtime sends an immediate report after WebSocket connection.
- [ ] Write failing tests: inbound backend messages are parsed and dispatched.
- [ ] Run `cargo test runtime` and verify the new tests fail because orchestration does not exist.
- [ ] Implement a single-cycle `run_once` runtime method for deterministic tests.
- [ ] Wire `main` to call the runtime entrypoint after parsing config.
- [ ] Run `cargo test runtime` and verify it passes.

### Task 6: Full Verification

**Files:**
- Modify: `README.md`

- [ ] Update README with milestone scope and local run command.
- [ ] Run `cargo fmt --check`.
- [ ] Run `cargo test`.
- [ ] Run `cargo run -- --endpoint https://panel.example.com --token secret-token-value --insecure --disable-web-ssh`.
- [ ] Confirm the command starts without exposing the raw token.
