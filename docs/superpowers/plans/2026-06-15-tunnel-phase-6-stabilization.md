# Tunnel Phase 6 Stabilization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the TCP tunnel MVP safe to run beyond a happy-path demo by fixing listener/session lifecycle and adding repeatable real-host validation.

**Architecture:** Keep the Phase 5 backend relay protocol unchanged. Stabilize the Rust agent runtime first: listener handles become stoppable, stale sessions are pruned, and validation runs through local tests plus a Linux-host test script.

**Tech Stack:** Rust stable std TCP/thread/channel primitives, tungstenite KTP frames, PowerShell/SSH for remote Linux verification, existing Go backend relay tests.

---

## File Structure

- Modify `src/tunnel_runtime.rs`: replace the listener `HashSet<u64>` with a managed listener map that can stop removed or changed rules; add session metadata for pruning.
- Modify `tests/tunnel_runtime.rs`: add tests for rule removal, listen port change, and session close cleanup.
- Create `scripts/tunnel-relay-local-smoke.sh`: a Linux-friendly smoke script documenting the two-agent relay simulation command.
- Create `tests/tunnel_relay_smoke_script.rs`: assert the smoke script exists, runs the tunnel runtime tests, and is valid shell when `bash` is present.

---

### Task 1: Stoppable Listener Lifecycle

**Files:**
- Modify: `tests/tunnel_runtime.rs`
- Modify: `src/tunnel_runtime.rs`

- [ ] **Step 1: Write failing removal test**

Add a test named `tcp_runtime_stops_listener_when_rule_is_removed` to `tests/tunnel_runtime.rs`:

```rust
#[test]
fn tcp_runtime_stops_listener_when_rule_is_removed() {
    let listen_port = free_tcp_port();
    let state = SharedTunnelRuleState::new();
    let mut rule = selected_rule(41, "tcp", "ingress", true);
    rule.listen_address = "127.0.0.1".to_string();
    rule.listen_port = listen_port;
    state.update_rules("rev-a", &[rule]);
    let mut runtime = TunnelTcpRuntime::new(state.clone());
    runtime.refresh_listeners().expect("start listener");
    assert!(TcpStream::connect(("127.0.0.1", listen_port)).is_ok());

    state.update_rules("rev-b", &[]);
    runtime.refresh_listeners().expect("stop removed listener");
    assert_port_eventually_closed(listen_port);
}
```

- [ ] **Step 2: Run RED**

Run:

```powershell
cargo test --test tunnel_runtime tcp_runtime_stops_listener_when_rule_is_removed -- --nocapture
```

Expected: failure because the old listener remains active after the rule is removed.

- [ ] **Step 3: Implement minimal stoppable listener handle**

Change `TunnelTcpRuntime.listeners` into a map of rule id to handle:

```rust
struct TcpListenerHandle {
    spec: TunnelTcpListenerSpec,
    stop: Arc<AtomicBool>,
}
```

Change `refresh_listeners` to stop handles missing from the new plan, restart handles whose listen address/port changed, and start handles for new specs. `start_tcp_listener` should receive `stop: Arc<AtomicBool>` and use a nonblocking accept loop so it can exit when `stop.load(Ordering::SeqCst)` is true.

- [ ] **Step 4: Run GREEN**

Run:

```powershell
cargo test --test tunnel_runtime tcp_runtime_stops_listener_when_rule_is_removed -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Add port-change coverage**

Add `tcp_runtime_restarts_listener_when_listen_port_changes` in `tests/tunnel_runtime.rs`; it starts a rule on port A, changes the same rule id to port B, calls `refresh_listeners`, then asserts A eventually closes and B accepts.

- [ ] **Step 6: Run listener suite**

Run:

```powershell
cargo test --test tunnel_runtime -- --nocapture
```

Expected: all tunnel runtime tests pass.

- [ ] **Step 7: Commit**

```powershell
git add src/tunnel_runtime.rs tests/tunnel_runtime.rs
git commit -m "fix: restart tunnel tcp listeners on rule changes"
```

---

### Task 2: Session Cleanup

**Files:**
- Modify: `tests/tunnel_runtime.rs`
- Modify: `src/tunnel_runtime.rs`

- [ ] **Step 1: Write failing cleanup test**

Add `tcp_runtime_removes_session_after_local_close` to `tests/tunnel_runtime.rs`; it opens an ingress client, closes the client socket, waits for a `SessionClose` frame, and asserts `runtime.active_session_count() == 0`.

- [ ] **Step 2: Run RED**

Run:

```powershell
cargo test --test tunnel_runtime tcp_runtime_removes_session_after_local_close -- --nocapture
```

Expected: failure because session metadata is not removed when the reader thread observes EOF.

- [ ] **Step 3: Implement cleanup hooks**

Add a shared session map cleanup path to `read_tcp_session`: after emitting `SessionClose` or `SessionError`, remove the session id from the shared map. Add `pub fn active_session_count(&self) -> usize` for tests and runtime diagnostics.

- [ ] **Step 4: Run GREEN**

Run:

```powershell
cargo test --test tunnel_runtime tcp_runtime_removes_session_after_local_close -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src/tunnel_runtime.rs tests/tunnel_runtime.rs
git commit -m "fix: cleanup tunnel tcp sessions after close"
```

---

### Task 3: Repeatable Linux Smoke Entry

**Files:**
- Create: `scripts/tunnel-relay-local-smoke.sh`
- Create: `tests/tunnel_relay_smoke_script.rs`

- [ ] **Step 1: Write failing script test**

Create `tests/tunnel_relay_smoke_script.rs`:

```rust
use std::process::Command;

#[test]
fn tunnel_relay_smoke_script_runs_runtime_relay_test() {
    let script = std::fs::read_to_string("scripts/tunnel-relay-local-smoke.sh")
        .expect("smoke script should be readable");
    assert!(script.contains("tcp_runtime_two_agent_relay_simulation_forwards_echo"));
    assert!(script.contains("cargo test --test tunnel_runtime"));
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
```

- [ ] **Step 2: Run RED**

Run:

```powershell
cargo test --test tunnel_relay_smoke_script -- --nocapture
```

Expected: failure because the script does not exist.

- [ ] **Step 3: Create script**

Create `scripts/tunnel-relay-local-smoke.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

cargo test --test tunnel_runtime tcp_runtime_two_agent_relay_simulation_forwards_echo -- --nocapture
cargo test --test tunnel_runtime tcp_runtime_stops_listener_when_rule_is_removed -- --nocapture
cargo test --test tunnel_runtime tcp_runtime_removes_session_after_local_close -- --nocapture
```

- [ ] **Step 4: Run GREEN**

Run:

```powershell
cargo test --test tunnel_relay_smoke_script -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add scripts/tunnel-relay-local-smoke.sh tests/tunnel_relay_smoke_script.rs
git commit -m "test: add tunnel relay smoke script"
```

---

### Task 4: Verification And Push

**Files:**
- No source changes expected.

- [ ] **Step 1: Local agent verification**

Run:

```powershell
cargo test
```

Expected: all tests pass.

- [ ] **Step 2: Backend relay verification**

Run in `kelicloud`:

```powershell
C:\Users\Administrator\Documents\tanzhen\.tools\go1.24.11\go\bin\go.exe test ./api/client -run "TestTunnelRelay|TestTunnelDataRelaySocketEncodesKTPFrames|TestHandleTunnelDataFrame|TestTunnelSession.*Payload" -count=1
C:\Users\Administrator\Documents\tanzhen\.tools\go1.24.11\go\bin\go.exe test ./api/client -run "^$" -count=1
```

Expected: both commands pass. Full SQLite-backed backend tests may still need a CGO-enabled Go environment.

- [ ] **Step 3: Linux remote verification**

Archive the latest `kelicloud-agent-rs` HEAD to `/tmp/kelicloud-agent-rs-phase6` on `2.56.116.39`, then run:

```bash
cd /tmp/kelicloud-agent-rs-phase6
cargo test --test tunnel_runtime --test tunnel_data --test tunnel_session --test tunnel_relay_smoke_script -- --nocapture
cargo build --release
```

Expected: tests and release build pass.

- [ ] **Step 4: Push**

Run:

```powershell
git push origin main
```

Expected: local `main` equals `origin/main`.
