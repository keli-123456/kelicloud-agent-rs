# Tunnel Data Transport Negotiation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add backwards-compatible tunnel control fields so the agent can advertise and receive data-plane transport choices before production traffic is switched from WebSocket to the encrypted KTP TCP carrier.

**Architecture:** Keep WebSocket as the default data transport. Extend the control Hello with supported data transports and extend selected rules with an optional `data_transport` field that defaults to `websocket` when old backends omit it. This prepares negotiation while keeping current WebSocket tunnel data behavior unchanged.

**Tech Stack:** Rust 2021, Serde JSON, existing tunnel control tests.

---

## File Structure

- Modify `src/tunnel_control.rs`
  - Add transport constants, supported transport helper, Hello `data_transports`, rule `data_transport`, defaulting helper, and `SelectedTunnelRule::data_transport`.
- Modify `tests/tunnel_control.rs`
  - Verify Hello advertises `websocket` and `ktp_tcp`.
  - Verify old RuleSync payloads default to `websocket`.
  - Verify new RuleSync payloads preserve `ktp_tcp`.
- Modify tests that construct `SelectedTunnelRule`
  - Add `data_transport: "websocket".to_string()` where needed.

## Task 1: Backwards-Compatible Negotiation Fields

**Files:**
- Modify: `src/tunnel_control.rs`
- Modify: `tests/tunnel_control.rs`
- Modify: selected-rule constructors in `tests/*.rs` and `src/tunnel_runtime.rs` tests.

- [ ] **Step 1: Write failing tests**

Add assertions to `tests/tunnel_control.rs`:

```rust
assert!(json.contains(r#""data_transports":["websocket","ktp_tcp"]"#));
```

Add tests:

```rust
#[test]
fn tunnel_control_defaults_missing_rule_data_transport_to_websocket() { ... }

#[test]
fn tunnel_control_parses_ktp_tcp_rule_data_transport() { ... }
```

- [ ] **Step 2: Run tests to verify failure**

Run:

```powershell
cargo test --test tunnel_control -- --nocapture
```

Expected: FAIL because the fields and helpers do not exist yet.

- [ ] **Step 3: Implement fields and defaults**

Add constants:

```rust
pub const TUNNEL_DATA_TRANSPORT_WEBSOCKET: &str = "websocket";
pub const TUNNEL_DATA_TRANSPORT_KTP_TCP: &str = "ktp_tcp";
```

Add `data_transports` to Hello, add `data_transport` to `SelectedTunnelRule` with `#[serde(default = "default_tunnel_data_transport")]`, and make `build_hello` include both transports.

- [ ] **Step 4: Run focused tests**

Run:

```powershell
cargo test --test tunnel_control -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Run regression**

Run:

```powershell
cargo test --test tunnel_control --test tunnel_data --test tunnel_runtime --test ktp_transport -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```powershell
git add src/tunnel_control.rs tests/tunnel_control.rs tests/tunnel_data.rs tests/tunnel_runtime.rs docs/superpowers/plans/2026-06-17-tunnel-data-transport-negotiation.md
git commit -m "feat: advertise ktp tcp tunnel data transport"
```
