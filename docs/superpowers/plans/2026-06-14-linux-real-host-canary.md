# Linux Real Host Canary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a repeatable real Linux host canary path for installing, upgrading, restarting, uninstalling, and documenting the Rust agent before default production rollout.

**Architecture:** Keep runtime behavior unchanged. Add a standalone Bash canary runner that wraps the released `install.sh` path and records non-secret evidence, then update docs and static tests so the release checklist can be executed consistently on a real systemd host.

**Tech Stack:** Bash, systemd, Rust static script tests, Markdown release evidence.

---

### Task 1: Canary Runner Contract

**Files:**
- Create: `scripts/canary-install.sh`
- Modify: `tests/canary_install_script.rs`

- [ ] **Step 1: Add a static test for required canary stages**

Create `tests/canary_install_script.rs`:

```rust
use std::path::PathBuf;

#[test]
fn canary_install_script_documents_real_host_stages() {
    let script = std::fs::read_to_string(canary_script_path()).unwrap();

    for expected in [
        "Real Linux host install canary",
        "--endpoint URL",
        "--auto-discovery KEY",
        "--install-version VERSION",
        "--rollback-command COMMAND",
        "install_agent",
        "verify_service",
        "restart_agent",
        "pin_or_upgrade_agent",
        "uninstall_agent",
        "run_rollback_command",
        "systemctl is-active",
        "journalctl -u kelicloud-agent-rs",
        "AGENT_ENDPOINT",
        "AGENT_AUTO_DISCOVERY_KEY",
        "kelicloud-agent-rs-linux",
    ] {
        assert!(script.contains(expected), "missing {expected}");
    }
}

fn canary_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("canary-install.sh")
}
```

- [ ] **Step 2: Run the targeted test and confirm it fails**

Run:

```bash
cargo test --locked --test canary_install_script -- --nocapture
```

Expected: FAIL because `scripts/canary-install.sh` does not exist yet.

- [ ] **Step 3: Create the canary runner**

Create `scripts/canary-install.sh` as a Linux-only Bash script that:

- Requires root, Linux, systemd, `curl`, and a real `systemctl`.
- Accepts `--endpoint`, `--auto-discovery`, optional `--install-version`, optional `--github-proxy`, optional `--duration`, optional `--keep-installed`, optional `--rollback-command`, optional `--rollback-service-name`, and optional `--skip-rollback-service-check`.
- Downloads and executes the released Rust installer from `https://raw.githubusercontent.com/keli-123456/kelicloud-agent-rs/refs/heads/main/install.sh`.
- Verifies `/usr/local/bin/kelicloud-agent-rs`, `/etc/kelicloud-agent-rs/config.env`, and `kelicloud-agent-rs.service`.
- Prints `kelicloud-agent-rs --version`, `systemctl is-active`, and a redacted config preview.
- Restarts the service and verifies it becomes active again.
- If `--install-version` is supplied, reruns install with that version to prove explicit pin/upgrade path.
- If `--keep-installed` is not supplied, uninstalls the Rust agent.
- If `--rollback-command` is supplied after uninstall, runs that exact command and waits for `kelicloud-agent.service` or `--rollback-service-name` to become active to prove Go-agent rollback.
- If `--evidence-file` is supplied, writes a Markdown evidence file with host,
  release asset, service, restart, upgrade, uninstall, rollback, and panel-side
  checklist fields.

- [ ] **Step 4: Run the targeted test and confirm it passes**

Run:

```bash
cargo test --locked --test canary_install_script -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit the runner**

Run:

```bash
git add scripts/canary-install.sh tests/canary_install_script.rs
git commit -m "Add Linux real host canary runner"
```

### Task 2: Canary Documentation

**Files:**
- Modify: `README.md`
- Modify: `docs/smoke-compatibility.md`

- [ ] **Step 1: Document how to run the canary**

Add README usage:

```bash
sudo bash scripts/canary-install.sh \
  --endpoint https://panel.example.com \
  --auto-discovery DISCOVERY_KEY \
  --install-version v0.1.0 \
  --keep-installed
```

Explain that panel-side exec, TCP ping, and WebSSH checks still need to be triggered while the service is online.

- [ ] **Step 2: Add a dated evidence template**

In `docs/smoke-compatibility.md`, add a blank real-host evidence template covering host, distro, arch, install command source, service status, panel online, exec, ping, terminal, restart, upgrade/pin, uninstall, rollback.

- [ ] **Step 3: Run docs/static checks**

Run:

```bash
git diff --check
cargo test --locked --test canary_install_script -- --nocapture
```

Expected: both pass.

- [ ] **Step 4: Commit docs**

Run:

```bash
git add README.md docs/smoke-compatibility.md
git commit -m "Document Linux real host canary flow"
```

### Task 3: Publish And Gate

**Files:**
- No source files beyond Task 1/2.

- [ ] **Step 1: Run full available verification**

Run:

```bash
cargo fmt --all -- --check
git diff --check
cargo test --locked --all-targets
```

Expected: all pass on the workstation. Unix-only installer behavior still relies on Linux CI.

- [ ] **Step 2: Push agent-rs**

Run:

```bash
git push origin main
```

Expected: push succeeds.

- [ ] **Step 3: Confirm GitHub CI and Local Backend Smoke**

Poll GitHub Actions for the pushed SHA.

Expected: `CI` and `Local Backend Smoke` complete successfully.

### Task 4: Real Host Execution Gate

**Files:**
- Modify after execution: `docs/smoke-compatibility.md`

- [ ] **Step 1: Select one expendable Linux systemd host**

Use a host where uninstall/reinstall is acceptable. Do not run this on an unknown production node without owner approval.

- [ ] **Step 2: Run the canary**

On the host:

```bash
git clone https://github.com/keli-123456/kelicloud-agent-rs.git
cd kelicloud-agent-rs
sudo bash scripts/canary-install.sh \
  --endpoint https://panel.example.com \
  --auto-discovery DISCOVERY_KEY \
  --install-version v0.1.0 \
  --keep-installed
```

- [ ] **Step 3: Trigger panel-side controls**

From kelicloud panel, verify this host:

- Appears online with sane metrics.
- Runs one script exec task and uploads stdout/stderr/exit code.
- Runs one TCP ping task.
- Opens one admin WebSSH terminal.

- [ ] **Step 4: Complete uninstall/rollback**

If rollback is required, rerun without `--keep-installed` and pass the Go agent rollback command:

```bash
sudo bash scripts/canary-install.sh \
  --endpoint https://panel.example.com \
  --auto-discovery DISCOVERY_KEY \
  --install-version v0.1.0 \
  --rollback-command '<panel generated Go agent install command>'
```

- [ ] **Step 5: Record evidence and commit**

Update `docs/smoke-compatibility.md` with the real host result, command source, run date, and any gaps.

Run:

```bash
git add docs/smoke-compatibility.md
git commit -m "Record Linux real host canary result"
git push origin main
```

---

## Self-Review

- Spec coverage: The plan covers install, online verification, exec, ping, terminal, restart, upgrade/pin, uninstall, rollback, docs, and CI gating.
- Placeholder scan: No implementation step relies on TBD behavior; the only human inputs are the actual endpoint, discovery key, target host, and optional rollback command.
- Type consistency: Script/test names and stage names match across tasks.
