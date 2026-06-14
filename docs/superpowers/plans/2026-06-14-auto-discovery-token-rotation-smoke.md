# Auto-Discovery Token Rotation Smoke Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the real local-backend smoke prove that `kelicloud-agent-rs` recovers after its auto-discovered backend token is rotated while the agent is alive.

**Architecture:** Extend the existing `scripts/smoke-local-backend.sh` path instead of adding a second workflow. The smoke will start the agent with backend auto-discovery, resolve the auto-created client through admin APIs, rotate the token through the real admin edit endpoint, wait for concrete recovery evidence, then run the existing CN/exec/ping/terminal checks against the recovered client.

**Tech Stack:** Bash smoke script, Rust script-structure tests, kelicloud admin HTTP APIs, GitHub Actions `Local Backend Smoke`.

---

### Task 1: Script Structure Tests

**Files:**
- Modify: `tests/local_backend_smoke_script.rs`

- [ ] **Step 1: Write the failing test assertions**

Add these assertions to `local_backend_smoke_script_orchestrates_real_backend_controls`:

```rust
assert!(script.contains("/api/admin/settings/"));
assert!(script.contains("AUTO_DISCOVERY_KEY"));
assert!(script.contains("--auto-discovery"));
assert!(script.contains("HOSTNAME=\"${SMOKE_AGENT_HOSTNAME}\""));
assert!(script.contains("/api/admin/client/list"));
assert!(script.contains("/api/admin/client/${CLIENT_UUID}/token"));
assert!(script.contains("/api/admin/client/${CLIENT_UUID}/edit"));
assert!(script.contains("rotate_auto_discovery_token"));
assert!(script.contains("wait_for_auto_discovery_recovery"));
assert!(script.contains("resolve_auto_discovery_client"));
assert!(script.contains("wait_for_log_count"));
assert!(script.contains("\"smoke: auto_discovery_registered\" 2"));
assert!(script.contains("\"smoke: token_recovered\" 1"));
assert!(!script.contains("--token \"${AGENT_TOKEN}\""));
```

- [ ] **Step 2: Verify the test fails**

Run:

```bash
cargo test --locked --test local_backend_smoke_script -- --nocapture
```

Expected: `local_backend_smoke_script_orchestrates_real_backend_controls` fails because the script does not yet contain the auto-discovery rotation stages.

### Task 2: Auto-Discovery Smoke Script Flow

**Files:**
- Modify: `scripts/smoke-local-backend.sh`
- Modify: `tests/local_backend_smoke_script.rs`

- [ ] **Step 1: Add script globals**

Near the existing global variables, add:

```bash
AUTO_DISCOVERY_KEY=""
SMOKE_AGENT_HOSTNAME="${SMOKE_AGENT_HOSTNAME:-agent-rs-smoke}"
SMOKE_AGENT_CLIENT_NAME="Auto-${SMOKE_AGENT_HOSTNAME}"
ROTATED_AGENT_TOKEN=""
```

- [ ] **Step 2: Add JSON payload support for token edit**

Extend `json_payload()` with:

```bash
elif kind == "client-token":
    print(json.dumps({"token": sys.argv[2]}))
```

- [ ] **Step 3: Add auto-discovery key loading**

Add this function after `login_admin()`:

```bash
load_auto_discovery_key() {
    local response
    response="$(curl_api GET "/api/admin/settings/")"
    AUTO_DISCOVERY_KEY="$(printf '%s' "${response}" | json_value "data.auto_discovery_key")"
    [[ -n "${AUTO_DISCOVERY_KEY}" ]] || die "settings response did not include auto_discovery_key"
    log "Loaded auto-discovery key for smoke"
}
```

- [ ] **Step 4: Add counted log waiting**

Add after `wait_for_log()`:

```bash
wait_for_log_count() {
    local file="$1"
    local needle="$2"
    local expected_count="$3"
    local timeout_seconds="$4"
    local deadline=$((SECONDS + timeout_seconds))
    local count
    until [[ -f "${file}" ]] && count="$(grep -F "${needle}" "${file}" | wc -l)" && (( count >= expected_count )); do
        if (( SECONDS >= deadline )); then
            if [[ -f "${file}" ]]; then
                tail -n 120 "${file}" >&2 || true
            fi
            die "timed out waiting for ${expected_count} log entries: ${needle}"
        fi
        sleep 1
    done
}
```

- [ ] **Step 5: Replace static client creation with auto-discovery client resolution**

Remove `create_client()` from the main path and add:

```bash
resolve_auto_discovery_client() {
    local response uuid token deadline
    deadline=$((SECONDS + 45))
    until response="$(curl_api GET "/api/admin/client/list" 2>/dev/null)" &&
        uuid="$(printf '%s' "${response}" | python3 - "${SMOKE_AGENT_CLIENT_NAME}" <<'PY'
import json
import sys

target = sys.argv[1]
try:
    data = json.load(sys.stdin)
except Exception:
    print("")
    raise SystemExit(0)

if isinstance(data, dict):
    clients = data.get("data", data)
else:
    clients = data

if not isinstance(clients, list):
    print("")
    raise SystemExit(0)

for client in reversed(clients):
    if isinstance(client, dict) and client.get("name") == target:
        print(client.get("uuid", ""))
        break
else:
    print("")
PY
)" && [[ -n "${uuid}" ]]; do
        if (( SECONDS >= deadline )); then
            die "timed out waiting for auto-discovered client ${SMOKE_AGENT_CLIENT_NAME}"
        fi
        sleep 1
    done

    CLIENT_UUID="${uuid}"
    response="$(curl_api GET "/api/admin/client/${CLIENT_UUID}/token")"
    token="$(printf '%s' "${response}" | json_value "token")"
    [[ -n "${token}" ]] || die "client token response did not include token"
    AGENT_TOKEN="${token}"
    log "Resolved auto-discovered smoke client ${CLIENT_UUID}"
}
```

- [ ] **Step 6: Start agent with auto-discovery**

In `start_agent()`, replace the static token argument:

```bash
HOSTNAME="${SMOKE_AGENT_HOSTNAME}" "${root}/target/release/kelicloud-agent-rs" \
    --endpoint "${BACKEND_ENDPOINT}" \
    --auto-discovery "${AUTO_DISCOVERY_KEY}" \
    --interval 1 \
    --max-retries 3 \
    --reconnect-interval 1 \
    --info-report-interval 1 >>"${AGENT_LOG}" 2>&1 &
```

- [ ] **Step 7: Add token rotation helper**

Add:

```bash
rotate_auto_discovery_token() {
    ROTATED_AGENT_TOKEN="rotated-${CLIENT_UUID}-${SECONDS}"
    local payload
    payload="$(json_payload client-token "${ROTATED_AGENT_TOKEN}")"
    curl_api POST "/api/admin/client/${CLIENT_UUID}/edit" "${payload}" >/dev/null
    log "Rotated auto-discovered client token through admin API"
}
```

- [ ] **Step 8: Add recovery wait helper**

Add:

```bash
wait_for_auto_discovery_recovery() {
    wait_for_log_count "${AGENT_LOG}" "smoke: token_recovered" 1 120
    wait_for_log_count "${AGENT_LOG}" "smoke: auto_discovery_registered" 2 120
    wait_for_log_count "${AGENT_LOG}" "smoke: report_websocket_connected" 2 120
    wait_for_log_count "${AGENT_LOG}" "smoke: report_sent" 2 120
    resolve_auto_discovery_client
}
```

- [ ] **Step 9: Update `main()` order**

Use this order:

```bash
set_stage "login admin"
login_admin
set_stage "load auto-discovery key"
load_auto_discovery_key
set_stage "start agent"
start_agent "${root}"
set_stage "resolve auto-discovered client"
resolve_auto_discovery_client
set_stage "rotate auto-discovery token"
rotate_auto_discovery_token
set_stage "wait for auto-discovery recovery"
wait_for_auto_discovery_recovery
set_stage "enable CN connectivity probe"
enable_cn_connectivity_probe
set_stage "trigger exec"
trigger_exec
set_stage "trigger ping"
trigger_ping
set_stage "trigger terminal"
trigger_terminal "${root}"
```

- [ ] **Step 10: Verify the script-structure test passes**

Run:

```bash
cargo test --locked --test local_backend_smoke_script -- --nocapture
```

Expected: all tests in `local_backend_smoke_script` pass.

### Task 3: Auto-Discovery Smoke Evidence

**Files:**
- Modify: `src/auto_discovery.rs`
- Modify: `src/token.rs`
- Modify: `tests/auto_discovery.rs`
- Modify: `tests/runtime.rs`
- Modify: `tests/local_backend_smoke_script.rs`

- [ ] **Step 1: Write a failing unit test for registration smoke evidence**

Add to `tests/auto_discovery.rs`:

```rust
#[test]
fn auto_discovery_registered_smoke_line_includes_uuid_only() {
    let cache = AutoDiscoveryCache {
        uuid: "registered-client".to_string(),
        token: "registered-token".to_string(),
    };

    assert_eq!(
        auto_discovery_registered_smoke_line(&cache),
        "smoke: auto_discovery_registered uuid=registered-client"
    );
}
```

- [ ] **Step 2: Verify the test fails**

Run:

```bash
cargo test --locked --test auto_discovery auto_discovery_registered_smoke_line_includes_uuid_only -- --nocapture
```

Expected: fail because `auto_discovery_registered_smoke_line` does not exist.

- [ ] **Step 3: Implement registration smoke evidence**

In `src/auto_discovery.rs`, add:

```rust
pub fn auto_discovery_registered_smoke_line(cache: &AutoDiscoveryCache) -> String {
    crate::smoke_summary::smoke_event_line("auto_discovery_registered", &[("uuid", &cache.uuid)])
}
```

After a successful register/save path in `resolve_auto_discovery_with`, emit:

```rust
println!("{}", auto_discovery_registered_smoke_line(&cache));
```

- [ ] **Step 4: Write a failing unit test for recovery smoke evidence**

Add to `tests/runtime.rs`:

```rust
#[test]
fn shared_token_recovery_emits_smoke_event_without_token_value() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let shared = SharedAgentToken::new("stale-token");
    let inner = FakeTokenRecovery::new(events.clone(), "fresh-token");
    let mut recovery = SharedTokenRecovery::new(inner, shared.clone());
    let mut config = base_config();
    config.token = "stale-token".to_string();

    let recovered = recovery.recover_from_transport_error(
        &mut config,
        &invalid_token_error("upload basic info", "stale-token"),
    );

    assert!(recovered);
    assert_eq!(shared.get(), "fresh-token");
    assert_eq!(
        token_recovered_smoke_line("upload basic info"),
        "smoke: token_recovered operation=upload_basic_info"
    );
}
```

Also import `SharedAgentToken`, `SharedTokenRecovery`, and `token_recovered_smoke_line` from `kelicloud_agent_rs::token`.

- [ ] **Step 5: Verify the recovery test fails**

Run:

```bash
cargo test --locked --test runtime shared_token_recovery_emits_smoke_event_without_token_value -- --nocapture
```

Expected: fail because `token_recovered_smoke_line` does not exist.

- [ ] **Step 6: Implement recovery smoke evidence**

In `src/token.rs`, add:

```rust
pub fn token_recovered_smoke_line(operation: &str) -> String {
    crate::smoke_summary::smoke_event_line("token_recovered", &[("operation", operation)])
}
```

In `SharedTokenRecovery::recover_from_transport_error`, after updating the shared token, emit:

```rust
if let TransportError::InvalidClientToken { operation, .. } = error {
    println!("{}", token_recovered_smoke_line(operation));
}
```

- [ ] **Step 7: Verify focused tests pass**

Run:

```bash
cargo test --locked --test auto_discovery auto_discovery_registered_smoke_line_includes_uuid_only -- --nocapture
cargo test --locked --test runtime shared_token_recovery_emits_smoke_event_without_token_value -- --nocapture
cargo test --locked --test local_backend_smoke_script -- --nocapture
```

Expected: all three commands pass.

### Task 4: Full Local Verification

**Files:**
- All modified files

- [ ] **Step 1: Run formatting**

Run:

```bash
cargo fmt --all -- --check
```

Expected: exit 0.

- [ ] **Step 2: Run whitespace check**

Run:

```bash
git diff --check
```

Expected: exit 0.

- [ ] **Step 3: Run all tests**

Run:

```bash
cargo test --locked --all-targets
```

Expected: exit 0.

- [ ] **Step 4: Commit implementation**

Run:

```bash
git add src/auto_discovery.rs src/token.rs tests/auto_discovery.rs tests/runtime.rs scripts/smoke-local-backend.sh tests/local_backend_smoke_script.rs
git commit -m "Smoke auto-discovery token rotation"
```

### Task 5: GitHub Real Backend Smoke

**Files:**
- Inspect GitHub workflow state. If the workflow fails, make a new failing test that reproduces the failing stage before changing code.

- [ ] **Step 1: Push to main**

Run:

```bash
git push origin main
```

- [ ] **Step 2: Poll GitHub Actions**

Check that both `CI` and `Local Backend Smoke` complete successfully for the pushed SHA.

Expected: `Local Backend Smoke` passes after running against real kelicloud backend and latest prepared web bundle.

- [ ] **Step 3: Inspect annotations on failure**

Use the GitHub Actions annotations to identify whether the failure is registration, token rotation, recovery wait, or post-recovery control-plane behavior. Fix with a new failing test first, then repeat Tasks 4 and 5.

### Task 6: Compatibility Documentation

**Files:**
- Modify: `docs/smoke-compatibility.md`

- [ ] **Step 1: Update compatibility notes**

Record:

- The passing SHA.
- The passing `Local Backend Smoke` run URL.
- The fact that auto-discovery startup, forced token rotation, re-registration, report recovery, and post-recovery exec/ping/terminal passed.
- Any remaining caveat, such as deletion/offline cleanup not being part of this smoke.

- [ ] **Step 2: Verify docs**

Run:

```bash
git diff --check
```

Expected: exit 0.

- [ ] **Step 3: Commit docs**

Run:

```bash
git add docs/smoke-compatibility.md
git commit -m "Document auto-discovery rotation smoke pass"
git push origin main
```

- [ ] **Step 4: Confirm latest smoke still passes**

Poll the latest `CI` and `Local Backend Smoke` runs for the docs SHA. The goal is complete only after both pass and the repository is clean.
