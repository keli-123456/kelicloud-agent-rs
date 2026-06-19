#!/usr/bin/env bash
set -Eeuo pipefail

SERVICE_NAME="kelicloud-agent-rs"
OLD_SERVICE_NAME="${KELICLOUD_OLD_AGENT_SERVICE:-komari-agent}"
REPO_RAW_BASE="${KELICLOUD_AGENT_RS_RAW_BASE:-https://raw.githubusercontent.com/keli-123456/kelicloud-agent-rs/refs/heads/main}"
INSTALL_URL="${REPO_RAW_BASE}/install.sh"
CANARY_URL="${REPO_RAW_BASE}/scripts/canary-install.sh"
CONTROL_URL="${REPO_RAW_BASE}/scripts/live-panel-control-smoke.sh"

ENDPOINT="${KELICLOUD_PANEL_ENDPOINT:-${AGENT_ENDPOINT:-}}"
AUTO_DISCOVERY_KEY="${KELICLOUD_CANARY_AUTO_DISCOVERY_KEY:-${AGENT_AUTO_DISCOVERY_KEY:-}}"
COOKIE_HEADER="${KELICLOUD_PANEL_COOKIE:-}"
COOKIE_JAR="${KELICLOUD_PANEL_COOKIE_JAR:-}"
PANEL_USERNAME="${KELICLOUD_PANEL_USERNAME:-}"
PANEL_PASSWORD="${KELICLOUD_PANEL_PASSWORD:-}"
PING_TARGET="${KELICLOUD_PANEL_PING_TARGET:-1.1.1.1:443}"
INSTALL_VERSION="${KELICLOUD_CANARY_INSTALL_VERSION:-}"
TUNNEL_KTP_TCP_ADDRESS="${KELICLOUD_CANARY_TUNNEL_KTP_TCP_ADDRESS:-${AGENT_TUNNEL_KTP_TCP_ADDRESS:-}}"
TUNNEL_KTP_TCP_AUTH_VERSION="${KELICLOUD_CANARY_TUNNEL_KTP_TCP_AUTH_VERSION:-${AGENT_TUNNEL_KTP_TCP_AUTH_VERSION:-}}"
TUNNEL_KTP_RELAY_BATCH_POLICY="${KELICLOUD_CANARY_TUNNEL_KTP_RELAY_BATCH_POLICY:-${AGENT_TUNNEL_KTP_RELAY_BATCH_POLICY:-}}"
SERVICE_WAIT_SECONDS="${KELICLOUD_CANARY_SERVICE_WAIT:-60}"
CONTROL_TIMEOUT_SECONDS="${KELICLOUD_PANEL_CONTROL_TIMEOUT:-90}"
WORKDIR="${KELICLOUD_CANARY_WORKDIR:-}"
ROLLBACK_COMMAND="${KELICLOUD_CANARY_ROLLBACK_COMMAND:-}"
SKIP_CONTROL="false"
STARTED_AT="$(date -u '+%Y-%m-%dT%H:%M:%SZ' 2>/dev/null || true)"
STARTED_EPOCH="$(date -u '+%s' 2>/dev/null || true)"
RUST_CLIENT_UUID=""
EVIDENCE_FILE=""
LOG_FILE=""
RESTORED="false"

usage() {
    cat <<'EOF'
Real Linux host control-plane canary for kelicloud-agent-rs.

Usage:
  sudo KELICLOUD_PANEL_COOKIE='session_token=...' \
    scripts/real-host-control-canary.sh --endpoint URL --auto-discovery KEY

Options:
  --endpoint URL              kelicloud panel endpoint
  --auto-discovery KEY        kelicloud auto-discovery key
  --cookie HEADER             raw admin Cookie header, also read from KELICLOUD_PANEL_COOKIE
  --cookie-jar PATH           curl cookie jar, also read from KELICLOUD_PANEL_COOKIE_JAR
  --username USERNAME         admin username, also read from KELICLOUD_PANEL_USERNAME
  --password PASSWORD         admin password, also read from KELICLOUD_PANEL_PASSWORD
  --ping-target HOST:PORT     TCP ping target, default 1.1.1.1:443
  --install-version VERSION   release tag to install/pin, default latest
  --service-wait SECONDS      wait time for Rust or rollback services, default 60
  --tunnel-ktp-tcp-address ADDRESS
                              enable KTP TCP tunnel data through this relay
  --tunnel-ktp-tcp-auth-version VERSION
                              KTP TCP auth version, v1 or v2
  --tunnel-ktp-relay-batch-policy POLICY
                              KTP relay batch policy, fixed or adaptive
  --old-service NAME          existing Go agent service to restore, default komari-agent
  --rollback-command COMMAND  command to run after Rust uninstall, default enables old service
  --workdir PATH              evidence/log directory
  --skip-control              install/restart/pin/rollback only; do not call live panel APIs
  --help                      Show this help

This wrapper downloads and runs canary-install.sh with --keep-installed, parses
the Rust client's smoke: auto_discovery_registered uuid=... journal evidence,
then runs live-panel-control-smoke.sh to trigger POST /api/admin/task/exec and
POST /api/admin/ping/add against the live panel. It always attempts to uninstall
kelicloud-agent-rs and restore the old komari-agent service before exiting.
EOF
}

log() {
    printf '%s\n' "$*"
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
            --cookie)
                need_value "$1" "${2:-}"
                COOKIE_HEADER="$2"
                shift 2
                ;;
            --cookie-jar)
                need_value "$1" "${2:-}"
                COOKIE_JAR="$2"
                shift 2
                ;;
            --username)
                need_value "$1" "${2:-}"
                PANEL_USERNAME="$2"
                shift 2
                ;;
            --password)
                need_value "$1" "${2:-}"
                PANEL_PASSWORD="$2"
                shift 2
                ;;
            --ping-target)
                need_value "$1" "${2:-}"
                PING_TARGET="$2"
                shift 2
                ;;
            --install-version)
                need_value "$1" "${2:-}"
                INSTALL_VERSION="$2"
                shift 2
                ;;
            --service-wait)
                need_value "$1" "${2:-}"
                SERVICE_WAIT_SECONDS="$2"
                shift 2
                ;;
            --tunnel-ktp-tcp-address|--ktp-tcp-address)
                need_value "$1" "${2:-}"
                TUNNEL_KTP_TCP_ADDRESS="$2"
                shift 2
                ;;
            --tunnel-ktp-tcp-auth-version|--ktp-tcp-auth-version)
                need_value "$1" "${2:-}"
                TUNNEL_KTP_TCP_AUTH_VERSION="$2"
                shift 2
                ;;
            --tunnel-ktp-relay-batch-policy|--ktp-relay-batch-policy)
                need_value "$1" "${2:-}"
                TUNNEL_KTP_RELAY_BATCH_POLICY="$2"
                shift 2
                ;;
            --old-service)
                need_value "$1" "${2:-}"
                OLD_SERVICE_NAME="$2"
                shift 2
                ;;
            --rollback-command)
                need_value "$1" "${2:-}"
                ROLLBACK_COMMAND="$2"
                shift 2
                ;;
            --workdir)
                need_value "$1" "${2:-}"
                WORKDIR="$2"
                shift 2
                ;;
            --skip-control)
                SKIP_CONTROL="true"
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
}

validate_config() {
    [[ "$(uname -s)" == "Linux" ]] || die "this canary must run on Linux"
    [[ "${EUID:-$(id -u)}" -eq 0 ]] || die "please run as root"
    [[ -n "$ENDPOINT" ]] || die "--endpoint or KELICLOUD_PANEL_ENDPOINT is required"
    [[ -n "$AUTO_DISCOVERY_KEY" ]] || die "--auto-discovery or KELICLOUD_CANARY_AUTO_DISCOVERY_KEY is required"
    [[ -n "$PING_TARGET" ]] || die "--ping-target or KELICLOUD_PANEL_PING_TARGET is required"
    if [[ "$SKIP_CONTROL" != "true" && -z "$COOKIE_HEADER" && -z "$COOKIE_JAR" && ( -z "$PANEL_USERNAME" || -z "$PANEL_PASSWORD" ) ]]; then
        die "--cookie, --cookie-jar, or --username/--password is required"
    fi
    case "$TUNNEL_KTP_TCP_AUTH_VERSION" in
        ""|v1|v2) ;;
        *) die "--tunnel-ktp-tcp-auth-version must be v1 or v2" ;;
    esac
    case "$TUNNEL_KTP_RELAY_BATCH_POLICY" in
        ""|fixed|adaptive) ;;
        *) die "--tunnel-ktp-relay-batch-policy must be fixed or adaptive" ;;
    esac
    if [[ -z "$TUNNEL_KTP_TCP_ADDRESS" && ( -n "$TUNNEL_KTP_TCP_AUTH_VERSION" || -n "$TUNNEL_KTP_RELAY_BATCH_POLICY" ) ]]; then
        die "--tunnel-ktp-tcp-address is required when KTP auth version or relay batch policy is set"
    fi
    command -v curl >/dev/null 2>&1 || die "curl is required"
    command -v systemctl >/dev/null 2>&1 || die "systemctl is required"
    command -v journalctl >/dev/null 2>&1 || die "journalctl is required"
}

setup_workdir() {
    if [[ -z "$WORKDIR" ]]; then
        WORKDIR="/root/kelicloud-agent-rs-canary-$(date -u +%Y%m%dT%H%M%SZ)-control"
    fi
    mkdir -p "$WORKDIR"
    LOG_FILE="${WORKDIR}/real-host-control-canary.log"
    EVIDENCE_FILE="${WORKDIR}/real-host-control-canary.evidence.md"
    exec > >(tee -a "$LOG_FILE") 2>&1
}

download_scripts() {
    curl -fsSL "$INSTALL_URL" -o "${WORKDIR}/install.sh"
    curl -fsSL "$CANARY_URL" -o "${WORKDIR}/canary-install.sh"
    curl -fsSL "$CONTROL_URL" -o "${WORKDIR}/live-panel-control-smoke.sh"
    chmod 0700 "${WORKDIR}/install.sh" "${WORKDIR}/canary-install.sh" "${WORKDIR}/live-panel-control-smoke.sh"
    bash -n "${WORKDIR}/install.sh"
    bash -n "${WORKDIR}/canary-install.sh"
    bash -n "${WORKDIR}/live-panel-control-smoke.sh"
}

parse_latest_registered_uuid() {
    local uuid
    uuid="$(journalctl -u "$SERVICE_NAME" --since "@${STARTED_EPOCH}" --no-pager 2>/dev/null |
        grep -F "smoke: auto_discovery_registered" |
        tail -n 1 |
        sed -n 's/.*[[:space:]]uuid=\([^[:space:]]*\).*/\1/p')"
    [[ -n "$uuid" ]] || die "could not find smoke: auto_discovery_registered uuid=... in ${SERVICE_NAME} journal"
    printf '%s\n' "$uuid"
}

wait_for_journal_evidence() {
    local since="$1"
    local needle="$2"
    local timeout="$3"
    local deadline=$((SECONDS + timeout))
    until journalctl -u "$SERVICE_NAME" --since "$since" --no-pager 2>/dev/null | grep -Fq "$needle"; do
        if (( SECONDS >= deadline )); then
            log "journalctl -u ${SERVICE_NAME} --since ${since} --no-pager"
            journalctl -u "$SERVICE_NAME" --since "$since" --no-pager 2>/dev/null | tail -n 120 || true
            die "timed out waiting for journal evidence: ${needle}"
        fi
        sleep 1
    done
}

wait_for_rust_report_websocket() {
    local since_epoch
    since_epoch="$(date -u '+%s' 2>/dev/null || true)"
    [[ -n "$since_epoch" ]] || since_epoch="$STARTED_EPOCH"

    log "==> wait for rust report websocket"
    systemctl restart "${SERVICE_NAME}.service"
    wait_for_journal_evidence "@${since_epoch}" "smoke: report_websocket_connected" "$SERVICE_WAIT_SECONDS"
    wait_for_journal_evidence "@${since_epoch}" "smoke: report_sent" "$SERVICE_WAIT_SECONDS"
    log "Rust report WebSocket connected and report sent."
}

run_install_canary() {
    log "==> canary install/restart/pin"
    log "Endpoint: ${ENDPOINT}"
    log "Auto-discovery key: $(redact_value "$AUTO_DISCOVERY_KEY")"
    log "Install version: ${INSTALL_VERSION}"
    if [[ -n "$TUNNEL_KTP_TCP_ADDRESS" ]]; then
        log "KTP TCP address: ${TUNNEL_KTP_TCP_ADDRESS}"
    fi
    if [[ -n "$TUNNEL_KTP_TCP_AUTH_VERSION" ]]; then
        log "KTP TCP auth version: ${TUNNEL_KTP_TCP_AUTH_VERSION}"
    fi
    if [[ -n "$TUNNEL_KTP_RELAY_BATCH_POLICY" ]]; then
        log "KTP relay batch policy: ${TUNNEL_KTP_RELAY_BATCH_POLICY}"
    fi
    systemctl stop "${OLD_SERVICE_NAME}.service" >/dev/null 2>&1 || true
    systemctl disable "${OLD_SERVICE_NAME}.service" >/dev/null 2>&1 || true
    local install_args=(
        --endpoint "$ENDPOINT" \
        --auto-discovery "$AUTO_DISCOVERY_KEY" \
        --duration 1 \
        --service-wait "$SERVICE_WAIT_SECONDS" \
        --keep-installed \
        --evidence-file "${WORKDIR}/real-host-canary.evidence.md"
    )
    if [[ -n "$INSTALL_VERSION" ]]; then
        install_args+=(--install-version "$INSTALL_VERSION")
    fi
    if [[ -n "$TUNNEL_KTP_TCP_ADDRESS" ]]; then
        install_args+=(--tunnel-ktp-tcp-address "$TUNNEL_KTP_TCP_ADDRESS")
    fi
    if [[ -n "$TUNNEL_KTP_TCP_AUTH_VERSION" ]]; then
        install_args+=(--tunnel-ktp-tcp-auth-version "$TUNNEL_KTP_TCP_AUTH_VERSION")
    fi
    if [[ -n "$TUNNEL_KTP_RELAY_BATCH_POLICY" ]]; then
        install_args+=(--tunnel-ktp-relay-batch-policy "$TUNNEL_KTP_RELAY_BATCH_POLICY")
    fi
    bash "${WORKDIR}/canary-install.sh" "${install_args[@]}"
    RUST_CLIENT_UUID="$(parse_latest_registered_uuid)"
    log "Rust client UUID: ${RUST_CLIENT_UUID}"
}

run_control_smoke() {
    if [[ "$SKIP_CONTROL" == "true" ]]; then
        log "Control-plane smoke skipped by --skip-control."
        return
    fi

    log "==> live panel control-plane smoke"
    wait_for_rust_report_websocket

    local args=(
        --endpoint "$ENDPOINT"
        --client "$RUST_CLIENT_UUID"
        --ping-target "$PING_TARGET"
        --timeout "$CONTROL_TIMEOUT_SECONDS"
        --journal-since "@${STARTED_EPOCH}"
    )
    if [[ -n "$COOKIE_HEADER" ]]; then
        KELICLOUD_PANEL_COOKIE="$COOKIE_HEADER" bash "${WORKDIR}/live-panel-control-smoke.sh" "${args[@]}"
    elif [[ -n "$COOKIE_JAR" ]]; then
        args+=(--cookie-jar "$COOKIE_JAR")
        bash "${WORKDIR}/live-panel-control-smoke.sh" "${args[@]}"
    else
        KELICLOUD_PANEL_USERNAME="$PANEL_USERNAME" \
            KELICLOUD_PANEL_PASSWORD="$PANEL_PASSWORD" \
            bash "${WORKDIR}/live-panel-control-smoke.sh" "${args[@]}"
    fi
}

write_evidence() {
    local status="$1"
    local finished_at
    finished_at="$(date -u '+%Y-%m-%dT%H:%M:%SZ' 2>/dev/null || true)"
    {
        printf '%s\n' '# kelicloud-agent-rs Real Host Control Canary Evidence'
        printf '\n'
        printf '%s\n' "- Result: \`${status}\`"
        printf '%s\n' "- Started at: \`${STARTED_AT:-unknown}\`"
        printf '%s\n' "- Finished at: \`${finished_at:-unknown}\`"
        printf '%s\n' "- Hostname: \`$(hostname 2>/dev/null || true)\`"
        if [[ -r /etc/os-release ]]; then
            . /etc/os-release
            printf '%s\n' "- Distro: \`${PRETTY_NAME:-unknown}\`"
        fi
        printf '%s\n' "- Kernel: \`$(uname -r 2>/dev/null || true)\`"
        printf '%s\n' "- Architecture: \`$(uname -m 2>/dev/null || true)\`"
        printf '%s\n' "- Panel endpoint: \`${ENDPOINT}\`"
        printf '%s\n' "- Install version: \`${INSTALL_VERSION}\`"
        printf '%s\n' "- KTP TCP address: \`${TUNNEL_KTP_TCP_ADDRESS:-not set}\`"
        printf '%s\n' "- KTP TCP auth version: \`${TUNNEL_KTP_TCP_AUTH_VERSION:-default}\`"
        printf '%s\n' "- KTP relay batch policy: \`${TUNNEL_KTP_RELAY_BATCH_POLICY:-default}\`"
        printf '%s\n' "- Rust client UUID: \`${RUST_CLIENT_UUID:-not resolved}\`"
        printf '%s\n' "- Ping target: \`${PING_TARGET}\`"
        printf '%s\n' "- Old service restored: \`${RESTORED}\`"
        printf '%s\n' "- Log file: \`${LOG_FILE}\`"
    } > "$EVIDENCE_FILE"
    log "Evidence file: ${EVIDENCE_FILE}"
}

restore_old_on_exit() {
    local status="$1"
    local evidence_status="passed"
    trap - EXIT
    log "==> restore old service, exit=${status}"
    if [[ -x "${WORKDIR:-}/install.sh" ]]; then
        bash "${WORKDIR}/install.sh" uninstall >/dev/null 2>&1 || true
    fi
    if [[ -n "$ROLLBACK_COMMAND" ]]; then
        bash -lc "$ROLLBACK_COMMAND" >/dev/null 2>&1 || true
    else
        systemctl enable --now "${OLD_SERVICE_NAME}.service" >/dev/null 2>&1 || true
    fi
    if systemctl is-active --quiet "${OLD_SERVICE_NAME}.service"; then
        RESTORED="true"
    fi
    printf 'old_active=%s\n' "$(systemctl is-active "${OLD_SERVICE_NAME}.service" 2>/dev/null || true)"
    printf 'old_enabled=%s\n' "$(systemctl is-enabled "${OLD_SERVICE_NAME}.service" 2>/dev/null || true)"
    printf 'rust_active=%s\n' "$(systemctl is-active "${SERVICE_NAME}.service" 2>/dev/null || true)"
    printf 'rust_enabled=%s\n' "$(systemctl is-enabled "${SERVICE_NAME}.service" 2>/dev/null || true)"
    if [[ "$status" -ne 0 ]]; then
        evidence_status="exit ${status}"
    fi
    write_evidence "$evidence_status"
    exit "$status"
}

main() {
    parse_args "$@"
    validate_config
    setup_workdir
    trap 'restore_old_on_exit "$?"' EXIT

    log "Real Linux host control-plane canary"
    log "Workdir: ${WORKDIR}"
    download_scripts
    run_install_canary
    run_control_smoke
}

main "$@"
