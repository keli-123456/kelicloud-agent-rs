# Linux Release Onboarding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `kelicloud-agent-rs` the default Linux install and upgrade path while preserving the current Go agent for Windows/macOS.

**Architecture:** Implement the release loop in three layers. First, make the Rust installer accept the panel's existing old-style install arguments and render complete Rust runtime environment files. Second, update backend-generated Linux auto-connect snippets to use the Rust script source by default. Third, update the web install and group-upgrade command builders so Linux uses Rust release assets and non-Linux platforms keep the Go agent path.

**Tech Stack:** Bash installer, Rust integration tests, Go backend helpers/tests, React/TypeScript command builders, GitHub Actions release/smoke workflows.

---

## File Structure

### `kelicloud-agent-rs`

- Modify `install.sh`: add panel-compatible aliases, auto-discovery env output, install-version/github-proxy mapping, Rust-supported metric flags, and uninstall cache cleanup.
- Modify `tests/install_script.rs`: add installer render tests for panel-compatible Linux commands and uninstall behavior.
- Modify `README.md`: document Linux panel command, explicit upgrade, rollback, and systemd-only limit.
- Modify `docs/smoke-compatibility.md`: add canary checklist status after implementation.

### `kelicloud`

- Modify `api/admin/install_script_source.go`: introduce explicit Go/Rust script source constants and a helper for flavor-specific script URLs.
- Modify `api/admin/install_script_source_test.go`: test Rust default source and legacy Go source normalization.
- Modify `api/admin/cloud_autoconnect.go`: generate Linux cloud auto-connect snippets from the Rust source.
- Modify `api/admin/cloud_autoconnect_test.go`: assert the cloud snippet uses Rust source and old-style Rust-compatible arguments.
- Modify `utils/failover/config.go`: generate automatic failover Linux install snippets from the Rust source.
- Modify `utils/failover/config_test.go`: assert failover snippets use Rust source and old-style Rust-compatible arguments.
- Modify `utils/failoverv2/autoconnect.go`: generate failover v2 Linux install snippets from the Rust source.
- Modify or create `utils/failoverv2/autoconnect_test.go`: assert failover v2 snippets use Rust source and old-style Rust-compatible arguments.

### `kelicloud-web`

- Modify `src/lib/installScriptSource.ts`: add Go/Rust source constants and flavor-aware URL builder.
- Create `src/lib/installScriptSource.test.mjs` only if the current unit test runner can pick it up; otherwise add static assertions to existing unit-test harness files.
- Modify `src/components/admin/node-details/GenerateCommandDialog.tsx`: Linux commands use Rust source and Rust cache path; Windows/macOS keep Go source.
- Modify `src/components/admin/node-details/GroupUpgradeDialog.tsx`: Linux release check uses `keli-123456/kelicloud-agent-rs` and Rust asset names; Windows/macOS keep Go release logic.
- Modify `src/components/admin/NodeTable/NodeFunction.tsx` only if it is still reachable for install command generation; otherwise leave it untouched.
- Modify `src/i18n/locales/zh_CN.json` and other locale JSON files only for new visible text that lacks a defaultValue fallback.

---

## Task 1: Agent Installer Compatibility

**Files:**

- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\install.sh`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\tests\install_script.rs`

- [ ] **Step 1: Write failing tests for panel-compatible aliases**

Append these tests to `tests/install_script.rs`:

```rust
#[cfg(unix)]
#[test]
fn render_env_accepts_panel_compatible_auto_discovery_arguments() {
    let output = std::process::Command::new("bash")
        .arg(install_script_path())
        .arg("-e")
        .arg("https://panel.example.com")
        .arg("--auto-discovery")
        .arg("discovery-key")
        .arg("--disable-web-ssh")
        .arg("--ignore-unsafe-cert")
        .arg("--memory-include-cache")
        .arg("--include-nics")
        .arg("eth0,eth1")
        .arg("--exclude-nics")
        .arg("lo")
        .arg("--include-mountpoint")
        .arg("/;/data")
        .arg("--month-rotate")
        .arg("3")
        .arg("render-env")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AGENT_ENDPOINT='https://panel.example.com'"));
    assert!(stdout.contains("AGENT_AUTO_DISCOVERY_KEY='discovery-key'"));
    assert!(stdout.contains("AGENT_DISABLE_WEB_SSH='true'"));
    assert!(stdout.contains("AGENT_INSECURE='true'"));
    assert!(stdout.contains("AGENT_MEMORY_INCLUDE_CACHE='true'"));
    assert!(stdout.contains("AGENT_INCLUDE_NICS='eth0,eth1'"));
    assert!(stdout.contains("AGENT_EXCLUDE_NICS='lo'"));
    assert!(stdout.contains("AGENT_INCLUDE_MOUNTPOINTS='/;/data'"));
    assert!(stdout.contains("AGENT_MONTH_ROTATE='3'"));
    assert!(!stdout.contains("AGENT_TOKEN="));
}

#[cfg(unix)]
#[test]
fn panel_style_command_defaults_to_install_when_command_is_omitted() {
    let output = std::process::Command::new("bash")
        .arg(install_script_path())
        .arg("-e")
        .arg("https://panel.example.com")
        .arg("--auto-discovery")
        .arg("discovery-key")
        .arg("--source-binary")
        .arg("/tmp/missing-agent-rs")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("source binary not found"),
        "stderr was {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn render_env_maps_install_version_and_github_proxy_aliases() {
    let output = std::process::Command::new("bash")
        .arg(install_script_path())
        .arg("render-env")
        .arg("-e")
        .arg("https://panel.example.com")
        .arg("-t")
        .arg("client-token")
        .arg("--install-version")
        .arg("v0.2.0")
        .arg("--install-ghproxy")
        .arg("https://ghfast.top")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AGENT_ENDPOINT='https://panel.example.com'"));
    assert!(stdout.contains("AGENT_TOKEN='client-token'"));
}
```

- [ ] **Step 2: Run the installer tests and verify red**

Run:

```powershell
cargo test --locked --test install_script -- --nocapture
```

Expected: the new tests fail because `install.sh` does not yet accept `-e`, `-t`, `--auto-discovery`, `--install-version`, `--install-ghproxy`, or panel-style command omission.

- [ ] **Step 3: Implement panel-compatible argument parsing**

In `install.sh`, add variables near the existing option variables:

```bash
AUTO_DISCOVERY_KEY=""
MEMORY_INCLUDE_CACHE=""
INCLUDE_NICS=""
EXCLUDE_NICS=""
INCLUDE_MOUNTPOINTS=""
MONTH_ROTATE=""
```

Update `usage()` so the top section includes:

```text
  install.sh -e URL (--token TOKEN | --auto-discovery KEY) [options]
```

Update install options to include:

```text
  -e, --endpoint URL            Backend endpoint, for AGENT_ENDPOINT
  -t, --token TOKEN             Client token, for AGENT_TOKEN
  --auto-discovery KEY          Auto-discovery key, for AGENT_AUTO_DISCOVERY_KEY
  --install-version VERSION     Alias for --version
  --install-ghproxy URL         Alias for --github-proxy
  --install-dir DIR             Install binary to DIR/kelicloud-agent-rs and config to DIR/config.env
  --ignore-unsafe-cert          Alias for --insecure
  --memory-include-cache        Set AGENT_MEMORY_INCLUDE_CACHE=true
  --include-nics CSV            Set AGENT_INCLUDE_NICS
  --exclude-nics CSV            Set AGENT_EXCLUDE_NICS
  --include-mountpoint LIST     Set AGENT_INCLUDE_MOUNTPOINTS
  --month-rotate DAY            Set AGENT_MONTH_ROTATE
```

Replace `parse_args()` with logic that accepts options before or after command:

```bash
parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            install|uninstall|restart|status|render-service|render-env)
                if [[ -z "${COMMAND:-}" ]]; then
                    COMMAND="$1"
                    shift
                else
                    die "multiple commands specified: ${COMMAND} and $1"
                fi
                ;;
            -e|--endpoint)
                need_value "$1" "${2:-}"
                ENDPOINT="$2"
                shift 2
                ;;
            -t|--token)
                need_value "$1" "${2:-}"
                TOKEN="$2"
                shift 2
                ;;
            --auto-discovery)
                need_value "$1" "${2:-}"
                AUTO_DISCOVERY_KEY="$2"
                shift 2
                ;;
            --source-binary)
                need_value "$1" "${2:-}"
                SOURCE_BINARY="$2"
                shift 2
                ;;
            --version|--install-version)
                need_value "$1" "${2:-}"
                VERSION="$2"
                shift 2
                ;;
            --github-proxy|--install-ghproxy)
                need_value "$1" "${2:-}"
                GITHUB_PROXY="${2%/}"
                shift 2
                ;;
            --install-dir)
                need_value "$1" "${2:-}"
                CONFIG_DIR="${2%/}"
                BIN_PATH="${CONFIG_DIR}/kelicloud-agent-rs"
                CONFIG_FILE="${CONFIG_DIR}/config.env"
                shift 2
                ;;
            --bin)
                need_value "$1" "${2:-}"
                BIN_PATH="$2"
                shift 2
                ;;
            --env)
                need_value "$1" "${2:-}"
                CONFIG_FILE="$2"
                CONFIG_DIR="$(dirname "$CONFIG_FILE")"
                shift 2
                ;;
            --disable-web-ssh)
                DISABLE_WEB_SSH="true"
                shift
                ;;
            --insecure|--ignore-unsafe-cert)
                INSECURE="true"
                shift
                ;;
            --interval)
                need_value "$1" "${2:-}"
                INTERVAL="$2"
                shift 2
                ;;
            --max-retries)
                need_value "$1" "${2:-}"
                MAX_RETRIES="$2"
                shift 2
                ;;
            --reconnect-interval)
                need_value "$1" "${2:-}"
                RECONNECT_INTERVAL="$2"
                shift 2
                ;;
            --info-report-interval)
                need_value "$1" "${2:-}"
                INFO_REPORT_INTERVAL="$2"
                shift 2
                ;;
            --cf-access-client-id)
                need_value "$1" "${2:-}"
                CF_ACCESS_CLIENT_ID="$2"
                shift 2
                ;;
            --cf-access-client-secret)
                need_value "$1" "${2:-}"
                CF_ACCESS_CLIENT_SECRET="$2"
                shift 2
                ;;
            --custom-dns)
                need_value "$1" "${2:-}"
                CUSTOM_DNS="$2"
                shift 2
                ;;
            --memory-include-cache)
                MEMORY_INCLUDE_CACHE="true"
                shift
                ;;
            --include-nics)
                need_value "$1" "${2:-}"
                INCLUDE_NICS="$2"
                shift 2
                ;;
            --exclude-nics)
                need_value "$1" "${2:-}"
                EXCLUDE_NICS="$2"
                shift 2
                ;;
            --include-mountpoint|--include-mountpoints)
                need_value "$1" "${2:-}"
                INCLUDE_MOUNTPOINTS="$2"
                shift 2
                ;;
            --month-rotate)
                need_value "$1" "${2:-}"
                MONTH_ROTATE="$2"
                shift 2
                ;;
            --keep-config)
                KEEP_CONFIG="true"
                shift
                ;;
            --help|-h)
                usage
                exit 0
                ;;
            *)
                die "unknown option: $1"
                ;;
        esac
    done

    SERVICE_FILE="/etc/systemd/system/${SERVICE_NAME}.service"
}
```

Update `main()`:

```bash
main() {
    COMMAND=""
    if [[ $# -eq 0 || "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
        usage
        exit 0
    fi
    parse_args "$@"
    if [[ -z "$COMMAND" ]]; then
        COMMAND="install"
    fi

    case "$COMMAND" in
        install) install_agent ;;
        uninstall) uninstall_agent ;;
        restart) restart_agent ;;
        status) status_agent ;;
        render-service) render_service ;;
        render-env) render_env ;;
        *) die "unknown command: $COMMAND" ;;
    esac
}
```

- [ ] **Step 4: Render all Rust runtime env values**

Update `render_env()`:

```bash
render_env() {
    emit_env "AGENT_ENDPOINT" "$ENDPOINT"
    emit_env "AGENT_TOKEN" "$TOKEN"
    emit_env "AGENT_AUTO_DISCOVERY_KEY" "$AUTO_DISCOVERY_KEY"
    if [[ "$DISABLE_WEB_SSH" == "true" ]]; then
        emit_env "AGENT_DISABLE_WEB_SSH" "true"
    fi
    emit_env "AGENT_INSECURE" "$INSECURE"
    emit_env "AGENT_INTERVAL" "$INTERVAL"
    emit_env "AGENT_MAX_RETRIES" "$MAX_RETRIES"
    emit_env "AGENT_RECONNECT_INTERVAL" "$RECONNECT_INTERVAL"
    emit_env "AGENT_INFO_REPORT_INTERVAL" "$INFO_REPORT_INTERVAL"
    emit_env "AGENT_CF_ACCESS_CLIENT_ID" "$CF_ACCESS_CLIENT_ID"
    emit_env "AGENT_CF_ACCESS_CLIENT_SECRET" "$CF_ACCESS_CLIENT_SECRET"
    emit_env "AGENT_CUSTOM_DNS" "$CUSTOM_DNS"
    emit_env "AGENT_MEMORY_INCLUDE_CACHE" "$MEMORY_INCLUDE_CACHE"
    emit_env "AGENT_INCLUDE_NICS" "$INCLUDE_NICS"
    emit_env "AGENT_EXCLUDE_NICS" "$EXCLUDE_NICS"
    emit_env "AGENT_INCLUDE_MOUNTPOINTS" "$INCLUDE_MOUNTPOINTS"
    emit_env "AGENT_MONTH_ROTATE" "$MONTH_ROTATE"
}
```

Update `write_config()` so either token or auto-discovery is acceptable:

```bash
write_config() {
    [[ -n "$ENDPOINT" ]] || die "--endpoint is required"
    if [[ -z "$TOKEN" && -z "$AUTO_DISCOVERY_KEY" ]]; then
        die "--token or --auto-discovery is required"
    fi
    mkdir -p "$CONFIG_DIR"
    render_env > "$CONFIG_FILE"
    chmod 0600 "$CONFIG_FILE"
}
```

- [ ] **Step 5: Clean up auto-discovery cache on uninstall**

In `uninstall_agent()`, after removing config file when `KEEP_CONFIG` is false, add:

```bash
rm -f "$(dirname "$BIN_PATH")/auto-discovery.json"
rm -f "${CONFIG_DIR}/auto-discovery.json"
```

Keep this inside the `KEEP_CONFIG != true` branch.

- [ ] **Step 6: Run installer tests and commit**

Run:

```powershell
cargo test --locked --test install_script -- --nocapture
cargo fmt --all -- --check
git diff --check
```

Expected: all commands exit `0`.

Commit:

```powershell
git add install.sh tests/install_script.rs
git commit -m "Support panel-compatible Rust installer args"
```

---

## Task 2: Agent Docs And Release Checklist

**Files:**

- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\README.md`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\docs\smoke-compatibility.md`

- [ ] **Step 1: Update README Linux Install section**

Replace the first install command with:

```bash
wget -qO- https://raw.githubusercontent.com/keli-123456/kelicloud-agent-rs/refs/heads/main/install.sh |
  sudo bash -s -- -e https://panel.example.com --auto-discovery DISCOVERY_KEY
```

Add this explicit token install command below it:

```bash
sudo ./install.sh install \
  --endpoint https://panel.example.com \
  --token TOKEN
```

Add this upgrade command:

```bash
wget -qO- https://raw.githubusercontent.com/keli-123456/kelicloud-agent-rs/refs/heads/main/install.sh |
  sudo bash -s -- -e https://panel.example.com --auto-discovery DISCOVERY_KEY --install-version v0.1.0
```

Add this rollback note:

```markdown
To roll back to the Go agent, uninstall the Rust service first:

```bash
sudo ./install.sh uninstall
```

Then run the Go agent install command generated by the panel for the same
server or group.
```
```

- [ ] **Step 2: Update smoke compatibility notes**

In `docs/smoke-compatibility.md`, add a `Linux Release Canary Checklist`
section with these unchecked manual items:

```markdown
## Linux Release Canary Checklist

- [ ] Generated Linux Rust install command installs `kelicloud-agent-rs`.
- [ ] The server appears online after auto-discovery.
- [ ] Script exec returns output and exit code.
- [ ] TCP ping returns a result.
- [ ] Admin WebSSH terminal opens and echoes input.
- [ ] `systemctl restart kelicloud-agent-rs` reconnects the node.
- [ ] Generated Linux Rust upgrade command reconnects the node.
- [ ] Rollback to the Go agent path is documented for operators.
```

- [ ] **Step 3: Run docs checks and commit**

Run:

```powershell
git diff --check
```

Expected: exit `0`.

Commit:

```powershell
git add README.md docs/smoke-compatibility.md
git commit -m "Document Linux Rust agent install loop"
```

---

## Task 3: Backend Rust Script Source Helpers

**Files:**

- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud\api\admin\install_script_source.go`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud\api\admin\install_script_source_test.go`

- [ ] **Step 1: Write failing tests for Rust source**

Keep the existing `TestBuildAgentInstallScriptURL` cases as the legacy Go-agent compatibility contract. Add a separate flavor-specific test:

```go
func TestBuildAgentInstallScriptURLForFlavor(t *testing.T) {
    if got := buildAgentInstallScriptURLForFlavor("", "install.sh", agentInstallFlavorRust); got != "https://raw.githubusercontent.com/keli-123456/kelicloud-agent-rs/refs/heads/main/install.sh" {
        t.Fatalf("rust install URL = %q", got)
    }
    if got := buildAgentInstallScriptURLForFlavor("", "install.sh", agentInstallFlavorGo); got != "https://raw.githubusercontent.com/keli-123456/kelicloud-agent/refs/heads/main/install.sh" {
        t.Fatalf("go install URL = %q", got)
    }
    if got := buildAgentInstallScriptURLForFlavor("https://cdn.example.com/legacy-go/", "install.sh", agentInstallFlavorRust); got != "https://raw.githubusercontent.com/keli-123456/kelicloud-agent-rs/refs/heads/main/install.sh" {
        t.Fatalf("rust install URL must ignore legacy custom base, got %q", got)
    }
}
```

- [ ] **Step 2: Run backend source tests and verify red**

Run:

```powershell
go test ./api/admin -run "TestBuildAgentInstallScriptURL|TestBuildAgentInstallScriptURLForFlavor"
```

Expected: the new flavor test fails to compile because `agentInstallFlavor`, `agentInstallFlavorRust`, `agentInstallFlavorGo`, and `buildAgentInstallScriptURLForFlavor` do not exist.

- [ ] **Step 3: Implement flavor-aware source helpers**

In `api/admin/install_script_source.go`, replace the single default constant with:

```go
type agentInstallFlavor string

const (
    agentInstallFlavorGo   agentInstallFlavor = "go"
    agentInstallFlavorRust agentInstallFlavor = "rust"

    defaultGoAgentInstallScriptBaseURL   = "https://raw.githubusercontent.com/keli-123456/kelicloud-agent/refs/heads/main"
    defaultRustAgentInstallScriptBaseURL = "https://raw.githubusercontent.com/keli-123456/kelicloud-agent-rs/refs/heads/main"
)
```

Add:

```go
func buildAgentInstallScriptURLForFlavor(baseScriptsURL, scriptFile string, flavor agentInstallFlavor) string {
    defaultBase := defaultGoAgentInstallScriptBaseURL
    if flavor == agentInstallFlavorRust {
        defaultBase = defaultRustAgentInstallScriptBaseURL
    }
    return buildAgentInstallScriptURLWithDefault(baseScriptsURL, scriptFile, defaultBase)
}
```

Change `buildAgentInstallScriptURL` to call the Go flavor:

```go
func buildAgentInstallScriptURL(baseScriptsURL, scriptFile string) string {
    return buildAgentInstallScriptURLForFlavor(baseScriptsURL, scriptFile, agentInstallFlavorGo)
}
```

Rename the old body to:

```go
func buildAgentInstallScriptURLWithDefault(baseScriptsURL, scriptFile, defaultBase string) string {
    base := normalizeAgentInstallScriptBaseURL(baseScriptsURL, defaultBase)
    scriptFile = strings.TrimLeft(strings.TrimSpace(scriptFile), "/")
    if scriptFile == "" {
        scriptFile = "install.sh"
    }
    return strings.TrimRight(base, "/") + "/" + scriptFile
}
```

Change `normalizeAgentInstallScriptBaseURL` signature:

```go
func normalizeAgentInstallScriptBaseURL(raw, defaultBase string) string {
    base := strings.TrimSpace(raw)
    if base == "" {
        return defaultBase
    }
    ...
}
```

- [ ] **Step 4: Run source tests and commit**

Run:

```powershell
go test ./api/admin -run "TestBuildAgentInstallScriptURL|TestBuildAgentInstallScriptURLForFlavor"
```

Expected: exit `0`.

Commit:

```powershell
git add api/admin/install_script_source.go api/admin/install_script_source_test.go
git commit -m "Add Rust agent install script source"
```

---

## Task 4: Backend Cloud Auto-Connect Uses Rust

**Files:**

- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud\api\admin\cloud_autoconnect.go`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud\api\admin\cloud_autoconnect_test.go`

- [ ] **Step 1: Write failing cloud snippet test**

Add this test:

```go
func TestBuildCloudAutoConnectInstallSnippetUsesRustInstaller(t *testing.T) {
    snippet := buildCloudAutoConnectInstallSnippet("user-a", "https://panel.example.com", "discovery-key")
    if !strings.Contains(snippet, "kelicloud-agent-rs/refs/heads/main/install.sh") {
        t.Fatalf("expected Rust installer source, got:\n%s", snippet)
    }
    if !strings.Contains(snippet, "bash \"$KOMARI_INSTALL_SCRIPT\" -e \"$KOMARI_ENDPOINT\" --auto-discovery \"$KOMARI_AUTO_DISCOVERY\"") {
        t.Fatalf("expected panel-compatible Rust install args, got:\n%s", snippet)
    }
}
```

- [ ] **Step 2: Run test and verify red**

Run:

```powershell
go test ./api/admin -run TestBuildCloudAutoConnectInstallSnippetUsesRustInstaller
```

Expected: fail because cloud auto-connect still uses the Go default source.

- [ ] **Step 3: Change cloud auto-connect to Rust source**

In `api/admin/cloud_autoconnect.go`, change:

```go
installScriptURL, err := resolveAgentInstallScriptURL(userUUID, "install.sh")
```

to:

```go
installScriptURL, err := resolveAgentInstallScriptURLForFlavor(userUUID, "install.sh", agentInstallFlavorRust)
```

Add helper in `api/admin/install_script_source.go` if not present:

```go
func resolveAgentInstallScriptURLForFlavor(userUUID, scriptFile string, flavor agentInstallFlavor) (string, error) {
    baseScriptsURL, err := config.GetAsForUser[string](userUUID, config.BaseScriptsURLKey, "")
    if err != nil {
        return "", fmt.Errorf("failed to load base scripts url: %w", err)
    }
    if flavor == agentInstallFlavorRust {
        return buildAgentInstallScriptURLForFlavor("", scriptFile, agentInstallFlavorRust), nil
    }
    return buildAgentInstallScriptURLForFlavor(baseScriptsURL, scriptFile, flavor), nil
}
```

This intentionally ignores `base_scripts_url` for the first Rust default so existing Go custom sources do not accidentally point Linux Rust installs at a Go script.

- [ ] **Step 4: Run cloud tests and commit**

Run:

```powershell
go test ./api/admin -run "TestBuildCloudAutoConnectInstallSnippetUsesRustInstaller|TestBuildAgentInstallScriptURL"
```

Expected: exit `0`.

Commit:

```powershell
git add api/admin/cloud_autoconnect.go api/admin/cloud_autoconnect_test.go api/admin/install_script_source.go
git commit -m "Use Rust agent for cloud auto-connect"
```

---

## Task 5: Backend Failover Auto-Connect Uses Rust

**Files:**

- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud\utils\failover\config.go`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud\utils\failover\config_test.go`
- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud\utils\failoverv2\autoconnect.go`
- Create or modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud\utils\failoverv2\autoconnect_test.go`

- [ ] **Step 1: Write failing failover v1 snippet test**

Append to `utils/failover/config_test.go`:

```go
func TestBuildInstallSnippetUsesRustInstaller(t *testing.T) {
    snippet := buildInstallSnippet(
        "https://raw.githubusercontent.com/keli-123456/kelicloud-agent-rs/refs/heads/main/install.sh",
        "https://panel.example.com",
        "discovery-key",
    )
    if !strings.Contains(snippet, "kelicloud-agent-rs/refs/heads/main/install.sh") {
        t.Fatalf("expected Rust installer URL, got:\n%s", snippet)
    }
    if !strings.Contains(snippet, "bash \"$KOMARI_INSTALL_SCRIPT\" -e \"$KOMARI_ENDPOINT\" --auto-discovery \"$KOMARI_AUTO_DISCOVERY\"") {
        t.Fatalf("expected panel-compatible Rust install args, got:\n%s", snippet)
    }
}
```

- [ ] **Step 2: Write failing failover v2 snippet test**

If `utils/failoverv2/autoconnect_test.go` does not exist, create it with this content:

```go
package failoverv2

import (
    "strings"
    "testing"
)

func TestBuildInstallSnippetUsesRustInstaller(t *testing.T) {
    snippet := buildInstallSnippet(
        "https://raw.githubusercontent.com/keli-123456/kelicloud-agent-rs/refs/heads/main/install.sh",
        "https://panel.example.com",
        "discovery-key",
    )
    if !strings.Contains(snippet, "kelicloud-agent-rs/refs/heads/main/install.sh") {
        t.Fatalf("expected Rust installer URL, got:\n%s", snippet)
    }
    if !strings.Contains(snippet, "bash \"$KOMARI_INSTALL_SCRIPT\" -e \"$KOMARI_ENDPOINT\" --auto-discovery \"$KOMARI_AUTO_DISCOVERY\"") {
        t.Fatalf("expected panel-compatible Rust install args, got:\n%s", snippet)
    }
}
```

- [ ] **Step 3: Run failover tests and verify current source gap**

Run:

```powershell
go test ./utils/failover ./utils/failoverv2 -run "TestBuildInstallSnippetUsesRustInstaller|TestDefaultAutoConnectGroup"
```

Expected before implementation: the direct default-source tests fail because both packages still resolve `install.sh` from the Go agent repository.

Add direct test to both packages:

```go
func TestDefaultAgentInstallScriptURLUsesRustAgent(t *testing.T) {
    got := buildAgentInstallScriptURL("", "install.sh")
    want := "https://raw.githubusercontent.com/keli-123456/kelicloud-agent-rs/refs/heads/main/install.sh"
    if got != want {
        t.Fatalf("expected %q, got %q", want, got)
    }
}
```

- [ ] **Step 4: Change failover defaults to Rust**

In both `utils/failover/config.go` and `utils/failoverv2/autoconnect.go`, change:

```go
defaultAgentInstallScriptBaseURL = "https://raw.githubusercontent.com/keli-123456/kelicloud-agent/refs/heads/main"
```

to:

```go
defaultAgentInstallScriptBaseURL = "https://raw.githubusercontent.com/keli-123456/kelicloud-agent-rs/refs/heads/main"
```

Keep the old-style install arguments in `buildInstallSnippet`; those arguments are supported by Task 1.

- [ ] **Step 5: Run failover tests and commit**

Run:

```powershell
go test ./utils/failover ./utils/failoverv2 -run "TestBuildInstallSnippetUsesRustInstaller|TestDefaultAgentInstallScriptURLUsesRustAgent|TestDefaultAutoConnectGroup"
```

Expected: exit `0`.

Commit:

```powershell
git add utils/failover/config.go utils/failover/config_test.go utils/failoverv2/autoconnect.go utils/failoverv2/autoconnect_test.go
git commit -m "Use Rust agent for failover auto-connect"
```

---

## Task 6: Web Flavor-Aware Install Script Sources

**Files:**

- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-web\src\lib\installScriptSource.ts`
- Test: existing web test harness or `npm run build`

- [ ] **Step 1: Add flavor-aware source helpers**

In `src/lib/installScriptSource.ts`, add:

```ts
export type AgentInstallFlavor = "go" | "rust";

export const GO_AGENT_INSTALL_SCRIPT_BASE =
  "https://raw.githubusercontent.com/keli-123456/kelicloud-agent/refs/heads/main";

export const RUST_AGENT_INSTALL_SCRIPT_BASE =
  "https://raw.githubusercontent.com/keli-123456/kelicloud-agent-rs/refs/heads/main";
```

Rename `DEFAULT_AGENT_INSTALL_SCRIPT_BASE` usage to `GO_AGENT_INSTALL_SCRIPT_BASE`.

Add:

```ts
export function defaultAgentInstallScriptBase(flavor: AgentInstallFlavor) {
  return flavor === "rust"
    ? RUST_AGENT_INSTALL_SCRIPT_BASE
    : GO_AGENT_INSTALL_SCRIPT_BASE;
}

export function normalizeAgentInstallScriptBaseForFlavor(
  baseScriptsUrl: string | undefined,
  flavor: AgentInstallFlavor,
) {
  if (flavor === "rust") {
    return RUST_AGENT_INSTALL_SCRIPT_BASE;
  }
  return normalizeAgentInstallScriptBase(baseScriptsUrl);
}

export function buildAgentInstallScriptURLForFlavor(
  baseScriptsUrl: string | undefined,
  scriptFile: string,
  flavor: AgentInstallFlavor,
) {
  const base = normalizeAgentInstallScriptBaseForFlavor(baseScriptsUrl, flavor);
  const file = String(scriptFile || "install.sh").replace(/^\/+/, "");
  return `${trimTrailingSlash(base)}/${file}`;
}
```

Keep existing `buildAgentInstallScriptURL` as the Go-compatible wrapper:

```ts
export function buildAgentInstallScriptURL(
  baseScriptsUrl: string | undefined,
  scriptFile: string,
) {
  return buildAgentInstallScriptURLForFlavor(baseScriptsUrl, scriptFile, "go");
}
```

- [ ] **Step 2: Run build for type feedback**

Run:

```powershell
npm run build
```

Expected at this point: the build succeeds if the helper is exported correctly and the legacy wrapper remains compatible.

Commit:

```powershell
git add src/lib/installScriptSource.ts
git commit -m "Add flavor-aware install script source helpers"
```

---

## Task 7: Web Linux Install Command Uses Rust

**Files:**

- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-web\src\components\admin\node-details\GenerateCommandDialog.tsx`

- [ ] **Step 1: Update imports and platform flavor decision**

Change import:

```ts
import {
  buildAgentInstallScriptURL,
  buildAgentInstallScriptURLForFlavor,
  type AgentInstallFlavor,
} from "@/lib/installScriptSource";
```

Add near `getDefaultInstallDir`:

```ts
const getAgentInstallFlavor = (platform: Platform): AgentInstallFlavor =>
  platform === "linux" ? "rust" : "go";
```

Update Linux default install dir:

```ts
case "linux":
  return "/opt/kelicloud-agent-rs";
```

- [ ] **Step 2: Generate Rust script URL for Linux**

Replace:

```ts
let scriptUrl = buildAgentInstallScriptURL(
  settings.base_scripts_url,
  scriptFile,
);
```

with:

```ts
const agentFlavor = getAgentInstallFlavor(selectedPlatform);
let scriptUrl =
  agentFlavor === "rust"
    ? buildAgentInstallScriptURLForFlavor(settings.base_scripts_url, "install.sh", "rust")
    : buildAgentInstallScriptURL(settings.base_scripts_url, scriptFile);
```

- [ ] **Step 3: Adjust Linux command for Rust installer**

For Linux, keep `-e` and `--auto-discovery` because Task 1 supports them.

When `groupMode && useAutoDiscovery`, keep the discovery cache path tied to the effective install directory:

```ts
`${effectiveInstallDir}/auto-discovery.json`
```

- [ ] **Step 4: Add visible Rust Linux label**

Under the platform segmented control, add:

```tsx
{selectedPlatform === "linux" ? (
  <p className="mt-2 text-xs leading-5 text-slate-500 dark:text-slate-400">
    {t("admin.nodeTable.rustAgentLinuxHint", {
      defaultValue:
        "Linux commands use the Rust Agent. Windows and macOS keep using the legacy Agent.",
    })}
  </p>
) : null}
```

- [ ] **Step 5: Run web build and commit**

Run:

```powershell
npm run build
```

Expected: exit `0`.

Commit:

```powershell
git add src/components/admin/node-details/GenerateCommandDialog.tsx
git commit -m "Use Rust agent for Linux install commands"
```

---

## Task 8: Web Linux Group Upgrade Uses Rust Release

**Files:**

- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-web\src\components\admin\node-details\GroupUpgradeDialog.tsx`

- [ ] **Step 1: Add flavor helpers**

Add near platform helpers:

```ts
type AgentInstallFlavor = "go" | "rust";

const getNodeAgentInstallFlavor = (node: NodeDetail): AgentInstallFlavor =>
  detectNodePlatform(node) === "linux" ? "rust" : "go";

const getAgentReleaseRepo = (flavor: AgentInstallFlavor) =>
  flavor === "rust"
    ? "keli-123456/kelicloud-agent-rs"
    : "keli-123456/kelicloud-agent";
```

- [ ] **Step 2: Return Rust asset names for Linux**

Change `normalizeAgentReleaseArch` so Linux ARMv7 maps to `armv7`, while Go non-Linux keeps current names:

```ts
const normalizeAgentReleaseArch = (
  value?: string | null,
  flavor: AgentInstallFlavor = "go",
) => {
  const arch = String(value || "").trim().toLowerCase();
  if (!arch) return null;
  if (arch === "amd64" || arch === "x86_64" || arch === "x64") return "amd64";
  if (arch === "arm64" || arch === "aarch64" || arch === "armv8" || arch === "armv8l") return "arm64";
  if (flavor === "rust" && (arch === "arm" || arch.startsWith("armv7") || arch.startsWith("armv6"))) return "armv7";
  if (arch === "386" || arch === "i386" || arch === "i686" || arch === "x86") return "386";
  if (arch === "arm" || arch.startsWith("armv7") || arch.startsWith("armv6")) return "arm";
  return null;
};
```

Change `buildAgentReleaseAssetNameCandidates`:

```ts
const buildAgentReleaseAssetNameCandidates = (node: NodeDetail) => {
  const flavor = getNodeAgentInstallFlavor(node);
  const arch = normalizeAgentReleaseArch(node.arch, flavor);
  if (!arch) return [];

  const platform = detectNodePlatform(node);
  if (flavor === "rust") {
    return [`kelicloud-agent-rs-linux-${arch}`];
  }
  if (platform === "windows") {
    return [`kelicloud-agent-windows-${arch}.exe`, `komari-agent-windows-${arch}.exe`];
  }
  if (platform === "macos") {
    return [`kelicloud-agent-darwin-${arch}`, `komari-agent-darwin-${arch}`];
  }
  return [`kelicloud-agent-linux-${arch}`, `komari-agent-linux-${arch}`];
};
```

- [ ] **Step 3: Resolve latest releases per flavor**

Replace `resolveLatestAgentUpgradeVersion` with:

```ts
const resolveLatestAgentUpgradeVersions = async (nodes: NodeDetail[]) => {
  const flavors = Array.from(new Set(nodes.map(getNodeAgentInstallFlavor)));
  const result: Record<AgentInstallFlavor, string> = { go: "", rust: "" };

  await Promise.all(
    flavors.map(async (flavor) => {
      const repo = getAgentReleaseRepo(flavor);
      const response = await fetch(`https://api.github.com/repos/${repo}/releases/latest`, {
        headers: { Accept: "application/vnd.github+json" },
        cache: "no-cache",
      });
      if (!response.ok) {
        throw new Error(formatApiErrorMessage(
          translate("admin.nodeTable.upgradeLatestReleaseFailed", {
            status: response.status,
            defaultValue: "Failed to load latest Agent release (GitHub HTTP {{status}})",
          }),
          { status: response.status },
        ));
      }

      const payload = (await response.json()) as GithubReleasePayload;
      const releaseTag = normalizeAgentReleaseTag(payload.tag_name || payload.name);
      if (!releaseTag) {
        throw new Error(formatApiErrorMessage(
          translate("admin.nodeTable.upgradeLatestReleaseTagUnavailable", {
            defaultValue: "Latest Agent release tag is unavailable",
          }),
        ));
      }

      const flavorNodes = nodes.filter((node) => getNodeAgentInstallFlavor(node) === flavor);
      const requiredAssetGroups = flavorNodes
        .map((node) => buildAgentReleaseAssetNameCandidates(node))
        .filter((candidates) => candidates.length > 0);
      const publishedAssets = new Set(
        (payload.assets || [])
          .map((asset) => String(asset?.name || "").trim())
          .filter(Boolean),
      );
      const missingAssets = requiredAssetGroups
        .filter((candidates) => !candidates.some((assetName) => publishedAssets.has(assetName)))
        .map((candidates) => candidates[0]);
      const uniqueMissingAssets = Array.from(new Set(missingAssets));
      if (uniqueMissingAssets.length > 0) {
        throw new Error(formatApiErrorMessage(
          `Agent release ${releaseTag} is not fully published yet. Missing assets: ${uniqueMissingAssets.join(", ")}`,
        ));
      }
      result[flavor] = releaseTag;
    }),
  );

  return result;
};
```

Update the caller:

```ts
const installVersions = await resolveLatestAgentUpgradeVersions(onlineNodes);
...
const command = buildAgentUpgradeCommand(
  node,
  settings,
  installVersions[getNodeAgentInstallFlavor(node)],
);
```

- [ ] **Step 4: Build Linux upgrade command with Rust source**

Import `buildAgentInstallScriptURLForFlavor`.

In `buildAgentUpgradeCommand`, for Linux use:

```ts
const agentFlavor = getNodeAgentInstallFlavor(node);
const scriptUrl =
  agentFlavor === "rust"
    ? buildAgentInstallScriptURLForFlavor(settings.base_scripts_url, "install.sh", "rust")
    : buildAgentInstallScriptURL(settings.base_scripts_url, "install.sh");
```

Keep `-e`, `-t`, and `--install-version`; Task 1 makes Rust installer accept these aliases.

- [ ] **Step 5: Run web build and commit**

Run:

```powershell
npm run build
```

Expected: exit `0`.

Commit:

```powershell
git add src/components/admin/node-details/GroupUpgradeDialog.tsx
git commit -m "Use Rust release for Linux group upgrades"
```

---

## Task 9: Cross-Repo Verification And Push Order

**Files:**

- All touched files in all three repositories.

- [ ] **Step 1: Verify agent repo**

Run in `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs`:

```powershell
cargo fmt --all -- --check
git diff --check
cargo test --locked --all-targets
```

Expected: all exit `0`.

- [ ] **Step 2: Verify backend repo**

Run in `C:\Users\Administrator\Documents\tanzhen\kelicloud`:

```powershell
go test ./api/admin ./utils/failover ./utils/failoverv2
```

Expected: all exit `0`.

- [ ] **Step 3: Verify web repo**

Run in `C:\Users\Administrator\Documents\tanzhen\kelicloud-web`:

```powershell
npm run build
```

Expected: exit `0`.

- [ ] **Step 4: Push agent-rs**

Run:

```powershell
git -C C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs status --short
git -C C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs push origin main
```

Then confirm GitHub Actions:

- `CI` passes.
- `Local Backend Smoke` passes.

- [ ] **Step 5: Push web**

Run:

```powershell
git -C C:\Users\Administrator\Documents\tanzhen\kelicloud-web status --short
git -C C:\Users\Administrator\Documents\tanzhen\kelicloud-web push origin main
```

Ignore the pre-existing untracked `product-design-audits/` directory unless it becomes part of the web change.

- [ ] **Step 6: Push backend with latest web trigger**

Run:

```powershell
git -C C:\Users\Administrator\Documents\tanzhen\kelicloud status --short
git -C C:\Users\Administrator\Documents\tanzhen\kelicloud push origin main
```

Confirm backend workflow prepares latest web bundle if that workflow exists for the repository. If no workflow status is available, run `scripts/prepare-frontend.sh` locally before the backend push and record the command output.

---

## Task 10: Release And Canary Evidence

**Files:**

- Modify: `C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs\docs\smoke-compatibility.md`

- [ ] **Step 1: Confirm or create a Rust release tag**

Check whether `keli-123456/kelicloud-agent-rs` has a latest release containing:

- `kelicloud-agent-rs-linux-amd64`
- `kelicloud-agent-rs-linux-arm64`
- `kelicloud-agent-rs-linux-armv7`

If no release exists, create a tag:

```powershell
git -C C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs tag v0.1.0
git -C C:\Users\Administrator\Documents\tanzhen\kelicloud-agent-rs push origin v0.1.0
```

Wait for the `Release` workflow to finish and confirm the three assets exist.

- [ ] **Step 2: Record canary checklist**

Update `docs/smoke-compatibility.md` with a dated canary evidence block:

```markdown
### YYYY-MM-DD Linux Release Canary

- Generated command source: Rust Linux install command from kelicloud-web main.
- Release version: `v0.1.0`.
- Host: Linux/systemd.
- Evidence:
  - [ ] Install command completed.
  - [ ] Node appeared online.
  - [ ] Script exec returned output and exit code.
  - [ ] TCP ping returned a result.
  - [ ] Admin WebSSH terminal opened and echoed input.
  - [ ] `systemctl restart kelicloud-agent-rs` reconnected.
  - [ ] Upgrade command completed and reconnected.
  - [ ] Rollback path documented.
```

Only check boxes that are actually verified on a real Linux canary. Leave unchecked boxes in place if they still require manual follow-up.

- [ ] **Step 3: Commit evidence**

Run:

```powershell
git diff --check
git add docs/smoke-compatibility.md
git commit -m "Record Linux Rust agent release canary"
git push origin main
```

Expected: push succeeds, and latest `CI` plus `Local Backend Smoke` pass again.

---

## Completion Criteria

The goal is complete only when all of these are true:

- `kelicloud-agent-rs/install.sh` accepts panel-compatible Linux install and upgrade arguments.
- A GitHub Release for `kelicloud-agent-rs` contains Linux amd64, arm64, and armv7 assets.
- Backend Linux auto-connect snippets use Rust source by default.
- Web Linux install commands use Rust source by default.
- Web Linux group upgrades check Rust releases and Rust asset names.
- Windows/macOS web paths still use the existing Go agent source and release checks.
- Agent repo verification passes locally and on GitHub.
- Backend targeted tests pass locally.
- Web build passes locally.
- Latest pushed `kelicloud-agent-rs` main has passing `CI` and `Local Backend Smoke`.
- Canary evidence is documented in `docs/smoke-compatibility.md`, with any manual gaps explicit and not implied as completed.
