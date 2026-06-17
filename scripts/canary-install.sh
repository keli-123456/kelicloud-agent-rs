#!/usr/bin/env bash
set -Eeuo pipefail

SERVICE_NAME="kelicloud-agent-rs"
BIN_PATH="/usr/local/bin/kelicloud-agent-rs"
CONFIG_FILE="/etc/kelicloud-agent-rs/config.env"
INSTALL_URL="https://raw.githubusercontent.com/keli-123456/kelicloud-agent-rs/refs/heads/main/install.sh"
REPO="keli-123456/kelicloud-agent-rs"

ENDPOINT="${AGENT_ENDPOINT:-}"
AUTO_DISCOVERY_KEY="${AGENT_AUTO_DISCOVERY_KEY:-}"
TUNNEL_KTP_TCP_ADDRESS="${AGENT_TUNNEL_KTP_TCP_ADDRESS:-}"
INSTALL_VERSION=""
GITHUB_PROXY=""
INSECURE="false"
DURATION_SECONDS="90"
SERVICE_WAIT_SECONDS="45"
KEEP_INSTALLED="false"
ROLLBACK_COMMAND=""
ROLLBACK_SERVICE_NAME="${KELICLOUD_ROLLBACK_SERVICE_NAME:-kelicloud-agent}"
SKIP_ROLLBACK_SERVICE_CHECK="false"
EVIDENCE_FILE="${KELICLOUD_CANARY_EVIDENCE_FILE:-}"
INSTALLER_PATH=""
STARTED_AT="$(date -u '+%Y-%m-%dT%H:%M:%SZ' 2>/dev/null || true)"
RELEASE_ASSET=""
RELEASE_ASSET_URL=""
RUST_SERVICE_STATUS="not checked"
ROLLBACK_SERVICE_STATUS="not checked"
INSTALL_RESULT="not run"
RESTART_RESULT="not run"
PIN_OR_UPGRADE_RESULT="not run"
UNINSTALL_RESULT="not run"
ROLLBACK_RESULT="not run"
EVIDENCE_WRITTEN="false"

usage() {
    cat <<'EOF'
Real Linux host install canary for kelicloud-agent-rs.

Usage:
  sudo bash scripts/canary-install.sh --endpoint URL --auto-discovery KEY [options]
  sudo AGENT_ENDPOINT=URL AGENT_AUTO_DISCOVERY_KEY=KEY bash scripts/canary-install.sh [options]

Required:
  --endpoint URL                 kelicloud panel endpoint, also read from AGENT_ENDPOINT
  --auto-discovery KEY           kelicloud auto-discovery key, also read from AGENT_AUTO_DISCOVERY_KEY

Options:
  --install-version VERSION      Re-run install with this release tag, for example v0.1.0
  --github-proxy URL             Prefix used by the installer for GitHub downloads
  --tunnel-ktp-tcp-address ADDRESS
                                  Enable KTP TCP tunnel data and pass the relay address
  --insecure                     Pass --ignore-unsafe-cert to the installer
  --duration SECONDS             Online observation window for panel exec/ping/WebSSH, default 90
  --service-wait SECONDS         Wait time for systemd active checks, default 45
  --keep-installed               Leave kelicloud-agent-rs installed at the end
  --rollback-command COMMAND     Run this panel-generated Go agent command after Rust uninstall
  --rollback-service-name NAME   Service expected after rollback, default kelicloud-agent
  --skip-rollback-service-check  Do not check rollback service status
  --evidence-file PATH           Write Markdown evidence, even when the canary fails
  --help                         Show this help

This script verifies the release asset name pattern kelicloud-agent-rs-linux-*,
the installed systemd service, AGENT_ENDPOINT / AGENT_AUTO_DISCOVERY_KEY config,
optional AGENT_TUNNEL_KTP_TCP_ADDRESS config, restart behavior, optional version
pin/upgrade, uninstall, and optional rollback.
EOF
}

log() {
    printf '%s\n' "$*"
}

stage() {
    printf '\n==> %s\n' "$*"
}

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

need_value() {
    local flag="$1"
    local value="${2:-}"
    if [[ -z "$value" ]]; then
        die "$flag requires a value"
    fi
}

redact_value() {
    local value="$1"
    local length="${#value}"
    if [[ "$length" -le 8 ]]; then
        printf '****'
    else
        printf '%s...%s' "${value:0:4}" "${value: -4}"
    fi
}

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --endpoint)
                need_value "$1" "${2:-}"
                ENDPOINT="$2"
                shift 2
                ;;
            --auto-discovery)
                need_value "$1" "${2:-}"
                AUTO_DISCOVERY_KEY="$2"
                shift 2
                ;;
            --install-version)
                need_value "$1" "${2:-}"
                INSTALL_VERSION="$2"
                shift 2
                ;;
            --tunnel-ktp-tcp-address|--ktp-tcp-address)
                need_value "$1" "${2:-}"
                TUNNEL_KTP_TCP_ADDRESS="$2"
                shift 2
                ;;
            --github-proxy)
                need_value "$1" "${2:-}"
                GITHUB_PROXY="${2%/}"
                shift 2
                ;;
            --insecure)
                INSECURE="true"
                shift
                ;;
            --duration)
                need_value "$1" "${2:-}"
                DURATION_SECONDS="$2"
                shift 2
                ;;
            --service-wait)
                need_value "$1" "${2:-}"
                SERVICE_WAIT_SECONDS="$2"
                shift 2
                ;;
            --keep-installed)
                KEEP_INSTALLED="true"
                shift
                ;;
            --rollback-command)
                need_value "$1" "${2:-}"
                ROLLBACK_COMMAND="$2"
                shift 2
                ;;
            --rollback-service-name)
                need_value "$1" "${2:-}"
                ROLLBACK_SERVICE_NAME="$2"
                shift 2
                ;;
            --skip-rollback-service-check)
                SKIP_ROLLBACK_SERVICE_CHECK="true"
                shift
                ;;
            --evidence-file)
                need_value "$1" "${2:-}"
                EVIDENCE_FILE="$2"
                shift 2
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
}

validate_config() {
    [[ -n "$ENDPOINT" ]] || die "--endpoint or AGENT_ENDPOINT is required"
    [[ -n "$AUTO_DISCOVERY_KEY" ]] || die "--auto-discovery or AGENT_AUTO_DISCOVERY_KEY is required"
    [[ "$DURATION_SECONDS" =~ ^[0-9]+$ ]] || die "--duration must be whole seconds"
    [[ "$SERVICE_WAIT_SECONDS" =~ ^[0-9]+$ ]] || die "--service-wait must be whole seconds"
    [[ "$DURATION_SECONDS" -gt 0 ]] || die "--duration must be greater than zero"
    [[ "$SERVICE_WAIT_SECONDS" -gt 0 ]] || die "--service-wait must be greater than zero"
    if [[ "$KEEP_INSTALLED" == "true" && -n "$ROLLBACK_COMMAND" ]]; then
        die "--keep-installed cannot be combined with --rollback-command"
    fi
    if [[ "$SKIP_ROLLBACK_SERVICE_CHECK" != "true" && -n "$ROLLBACK_COMMAND" && -z "$ROLLBACK_SERVICE_NAME" ]]; then
        die "--rollback-service-name cannot be empty"
    fi
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || die "$1 command is required"
}

require_linux_systemd() {
    [[ "$(uname -s)" == "Linux" ]] || die "this canary must run on Linux"
    [[ "${EUID:-$(id -u)}" -eq 0 ]] || die "please run as root"
    require_command curl
    require_command systemctl
    require_command journalctl
    if [[ ! -d /run/systemd/system ]]; then
        die "a real systemd host is required; /run/systemd/system is missing"
    fi
}

detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64) printf 'amd64' ;;
        aarch64|arm64) printf 'arm64' ;;
        armv7l|armv7*) printf 'armv7' ;;
        *) die "unsupported architecture: $(uname -m)" ;;
    esac
}

release_asset_name() {
    printf 'kelicloud-agent-rs-linux-%s' "$(detect_arch)"
}

download_url_for_asset() {
    local asset="$1"
    local version_path="latest/download"
    if [[ -n "$INSTALL_VERSION" ]]; then
        version_path="download/${INSTALL_VERSION}"
    fi
    local url="https://github.com/${REPO}/releases/${version_path}/${asset}"
    if [[ -n "$GITHUB_PROXY" ]]; then
        printf '%s/%s' "$GITHUB_PROXY" "$url"
    else
        printf '%s' "$url"
    fi
}

verify_release_asset() {
    local asset url
    asset="$(release_asset_name)"
    url="$(download_url_for_asset "$asset")"
    RELEASE_ASSET="$asset"
    RELEASE_ASSET_URL="$url"
    stage "verify release asset"
    log "Expected asset: ${asset}"
    log "Checking download URL: ${url}"
    curl -fsIL "$url" >/dev/null || die "release asset is not reachable: ${asset}"
}

download_installer() {
    INSTALLER_PATH="$(mktemp "${TMPDIR:-/tmp}/kelicloud-agent-rs-install.XXXXXX.sh")"
    curl -fsSL "$INSTALL_URL" -o "$INSTALLER_PATH"
    chmod 0700 "$INSTALLER_PATH"
}

installer_args() {
    INSTALL_ARGS=(
        -e "$ENDPOINT"
        --auto-discovery "$AUTO_DISCOVERY_KEY"
    )
    if [[ -n "$INSTALL_VERSION" ]]; then
        INSTALL_ARGS+=(--install-version "$INSTALL_VERSION")
    fi
    if [[ -n "$TUNNEL_KTP_TCP_ADDRESS" ]]; then
        INSTALL_ARGS+=(--enable-tunnel-data --tunnel-ktp-tcp-address "$TUNNEL_KTP_TCP_ADDRESS")
    fi
    if [[ -n "$GITHUB_PROXY" ]]; then
        INSTALL_ARGS+=(--install-ghproxy "$GITHUB_PROXY")
    fi
    if [[ "$INSECURE" == "true" ]]; then
        INSTALL_ARGS+=(--ignore-unsafe-cert)
    fi
}

install_agent() {
    stage "install_agent"
    installer_args
    bash "$INSTALLER_PATH" "${INSTALL_ARGS[@]}"
    INSTALL_RESULT="passed"
}

wait_for_service() {
    local deadline=$((SECONDS + SERVICE_WAIT_SECONDS))
    until systemctl is-active --quiet "${SERVICE_NAME}.service"; do
        if (( SECONDS >= deadline )); then
            log "systemctl is-active ${SERVICE_NAME}.service: $(systemctl is-active "${SERVICE_NAME}.service" 2>/dev/null || true)"
            log "journalctl -u kelicloud-agent-rs -n 80 --no-pager"
            journalctl -u kelicloud-agent-rs -n 80 --no-pager || true
            die "${SERVICE_NAME}.service did not become active"
        fi
        sleep 1
    done
}

print_config_preview() {
    if [[ ! -f "$CONFIG_FILE" ]]; then
        die "config file missing: ${CONFIG_FILE}"
    fi
    log "Config preview:"
    sed -E \
        -e "s/^(AGENT_TOKEN=).*/\1'[redacted]'/" \
        -e "s/^(AGENT_AUTO_DISCOVERY_KEY=).*/\1'[redacted]'/" \
        -e "s/^(AGENT_CF_ACCESS_CLIENT_SECRET=).*/\1'[redacted]'/" \
        "$CONFIG_FILE"
}

verify_service() {
    stage "verify_service"
    [[ -x "$BIN_PATH" ]] || die "binary missing or not executable: ${BIN_PATH}"
    [[ -f "$CONFIG_FILE" ]] || die "config missing: ${CONFIG_FILE}"
    [[ -f "/etc/systemd/system/${SERVICE_NAME}.service" ]] || die "systemd unit missing"
    grep -q "^AGENT_ENDPOINT=" "$CONFIG_FILE" || die "AGENT_ENDPOINT missing from config"
    grep -q "^AGENT_AUTO_DISCOVERY_KEY=" "$CONFIG_FILE" || die "AGENT_AUTO_DISCOVERY_KEY missing from config"
    if [[ -n "$TUNNEL_KTP_TCP_ADDRESS" ]]; then
        grep -q "^AGENT_TUNNEL_DATA_ENABLED='true'" "$CONFIG_FILE" ||
            die "AGENT_TUNNEL_DATA_ENABLED missing from config"
        grep -q "^AGENT_TUNNEL_KTP_TCP_ADDRESS=" "$CONFIG_FILE" ||
            die "AGENT_TUNNEL_KTP_TCP_ADDRESS missing from config"
    fi
    wait_for_service
    RUST_SERVICE_STATUS="$(systemctl is-active "${SERVICE_NAME}.service")"
    log "systemctl is-active ${SERVICE_NAME}.service: ${RUST_SERVICE_STATUS}"
    log "Binary: ${BIN_PATH}"
    log "Config: ${CONFIG_FILE}"
    print_config_preview
}

restart_agent() {
    stage "restart_agent"
    systemctl restart "${SERVICE_NAME}.service"
    wait_for_service
    RESTART_RESULT="passed"
    log "Restart verified."
}

pin_or_upgrade_agent() {
    if [[ -z "$INSTALL_VERSION" ]]; then
        stage "pin_or_upgrade_agent"
        log "Skipped: pass --install-version VERSION to verify an explicit release pin or upgrade."
        PIN_OR_UPGRADE_RESULT="skipped"
        return
    fi

    stage "pin_or_upgrade_agent"
    installer_args
    bash "$INSTALLER_PATH" "${INSTALL_ARGS[@]}"
    wait_for_service
    PIN_OR_UPGRADE_RESULT="passed: ${INSTALL_VERSION}"
    log "Pinned or upgraded to requested release: ${INSTALL_VERSION}"
}

observe_panel_window() {
    stage "panel observation window"
    log "Keep this host selected in kelicloud now."
    log "Trigger one script exec task, one TCP ping task, and one WebSSH terminal before this window ends."
    log "Observation window: ${DURATION_SECONDS}s"
    sleep "$DURATION_SECONDS"
}

uninstall_agent() {
    stage "uninstall_agent"
    bash "$INSTALLER_PATH" uninstall
    if systemctl list-unit-files "${SERVICE_NAME}.service" >/dev/null 2>&1 &&
        systemctl list-unit-files "${SERVICE_NAME}.service" | grep -q "${SERVICE_NAME}.service"; then
        die "systemd unit still appears after uninstall"
    fi
    [[ ! -e "$BIN_PATH" ]] || die "binary still exists after uninstall: ${BIN_PATH}"
    [[ ! -e "$CONFIG_FILE" ]] || die "config still exists after uninstall: ${CONFIG_FILE}"
    UNINSTALL_RESULT="passed"
    RUST_SERVICE_STATUS="uninstalled"
    log "Rust agent uninstall verified."
}

run_rollback_command() {
    if [[ -z "$ROLLBACK_COMMAND" ]]; then
        return
    fi

    stage "run_rollback_command"
    log "Running the supplied panel-generated rollback command."
    bash -lc "$ROLLBACK_COMMAND"
    ROLLBACK_RESULT="command passed"
    verify_rollback_service
}

verify_rollback_service() {
    if [[ "$SKIP_ROLLBACK_SERVICE_CHECK" == "true" ]]; then
        ROLLBACK_SERVICE_STATUS="skipped"
        log "Rollback service check skipped."
        return
    fi

    stage "verify_rollback_service"
    local deadline=$((SECONDS + SERVICE_WAIT_SECONDS))
    until systemctl is-active --quiet "${ROLLBACK_SERVICE_NAME}.service"; do
        if (( SECONDS >= deadline )); then
            log "systemctl is-active ${ROLLBACK_SERVICE_NAME}.service: $(systemctl is-active "${ROLLBACK_SERVICE_NAME}.service" 2>/dev/null || true)"
            log "journalctl -u ${ROLLBACK_SERVICE_NAME} -n 80 --no-pager"
            journalctl -u "${ROLLBACK_SERVICE_NAME}" -n 80 --no-pager || true
            die "rollback service did not become active: ${ROLLBACK_SERVICE_NAME}.service"
        fi
        sleep 1
    done
    ROLLBACK_SERVICE_STATUS="$(systemctl is-active "${ROLLBACK_SERVICE_NAME}.service")"
    ROLLBACK_RESULT="passed"
    log "Rollback service active: ${ROLLBACK_SERVICE_NAME}.service"
}

cleanup() {
    if [[ -n "$INSTALLER_PATH" && -f "$INSTALLER_PATH" ]]; then
        rm -f "$INSTALLER_PATH"
    fi
}

shell_output_or_empty() {
    "$@" 2>/dev/null || true
}

os_pretty_name() {
    if [[ -r /etc/os-release ]]; then
        . /etc/os-release
        printf '%s' "${PRETTY_NAME:-unknown}"
    else
        printf 'unknown'
    fi
}

write_evidence() {
    local status="$1"
    [[ -n "$EVIDENCE_FILE" ]] || return 0
    mkdir -p "$(dirname "$EVIDENCE_FILE")"
    local finished_at
    finished_at="$(date -u '+%Y-%m-%dT%H:%M:%SZ' 2>/dev/null || true)"

    {
        printf '%s\n' '# kelicloud-agent-rs Real Host Canary Evidence'
        printf '\n'
        printf '%s\n' '- Result: '"\`${status}\`"
        printf '%s\n' "- Started at: \`${STARTED_AT:-unknown}\`"
        printf '%s\n' "- Finished at: \`${finished_at:-unknown}\`"
        printf '%s\n' "- Hostname: \`$(shell_output_or_empty hostname)\`"
        printf '%s\n' "- Distro: \`$(os_pretty_name)\`"
        printf '%s\n' "- Kernel: \`$(shell_output_or_empty uname -r)\`"
        printf '%s\n' "- Architecture: \`$(shell_output_or_empty uname -m)\`"
        printf '%s\n' "- Panel endpoint: \`${ENDPOINT}\`"
        printf '%s\n' "- Install source: \`${INSTALL_URL}\`"
        printf '%s\n' "- Requested install version: \`${INSTALL_VERSION:-latest}\`"
        printf '%s\n' "- Release asset: \`${RELEASE_ASSET:-not checked}\`"
        printf '%s\n' "- Release asset URL: \`${RELEASE_ASSET_URL:-not checked}\`"
        printf '%s\n' "- Rust install result: \`${INSTALL_RESULT}\`"
        printf '%s\n' "- Rust service status: \`${RUST_SERVICE_STATUS}\`"
        printf '%s\n' "- Rust restart result: \`${RESTART_RESULT}\`"
        printf '%s\n' "- Explicit install-version pin/upgrade result: \`${PIN_OR_UPGRADE_RESULT}\`"
        printf '%s\n' "- Rust uninstall result: \`${UNINSTALL_RESULT}\`"
        printf '%s\n' "- Go-agent rollback command result: \`${ROLLBACK_RESULT}\`"
        printf '%s\n' "- Go-agent rollback service name: \`${ROLLBACK_SERVICE_NAME}\`"
        printf '%s\n' "- Go-agent rollback service status: \`${ROLLBACK_SERVICE_STATUS}\`"
        printf '%s\n' '- Panel-side checks required: online metrics, script exec, TCP ping, and WebSSH terminal.'
        printf '\n## Operator Notes\n\n'
        printf '%s\n' '- Panel online and metrics:'
        printf '%s\n' '- Script exec task result:'
        printf '%s\n' '- TCP ping task result:'
        printf '%s\n' '- Admin WebSSH terminal result:'
        printf '%s\n' '- Remaining gaps or rollout notes:'
    } > "$EVIDENCE_FILE"
    EVIDENCE_WRITTEN="true"
    log "Evidence file: ${EVIDENCE_FILE}"
}

on_exit() {
    local status="$?"
    if [[ "$EVIDENCE_WRITTEN" != "true" && -n "$EVIDENCE_FILE" ]]; then
        write_evidence "exit ${status}" || true
    fi
    cleanup
    exit "$status"
}
trap on_exit EXIT

main() {
    parse_args "$@"
    validate_config
    require_linux_systemd

    stage "canary context"
    log "Endpoint: ${ENDPOINT}"
    log "Auto-discovery key: $(redact_value "$AUTO_DISCOVERY_KEY")"
    if [[ -n "$TUNNEL_KTP_TCP_ADDRESS" ]]; then
        log "KTP TCP address: ${TUNNEL_KTP_TCP_ADDRESS}"
    fi
    log "Install source: ${INSTALL_URL}"
    if [[ -n "$INSTALL_VERSION" ]]; then
        log "Install version: ${INSTALL_VERSION}"
    else
        log "Install version: latest"
    fi

    verify_release_asset
    download_installer
    install_agent
    verify_service
    restart_agent
    pin_or_upgrade_agent
    observe_panel_window

    if [[ "$KEEP_INSTALLED" == "true" ]]; then
        stage "keep installed"
        log "Leaving ${SERVICE_NAME} installed for longer canary observation."
    else
        uninstall_agent
        run_rollback_command
    fi

    stage "canary finished"
    write_evidence "passed"
    log "Record panel-side exec, TCP ping, WebSSH, restart, install, upgrade, uninstall, and rollback evidence in docs/smoke-compatibility.md."
}

main "$@"
