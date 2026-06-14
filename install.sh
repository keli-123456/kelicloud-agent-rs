#!/usr/bin/env bash
set -euo pipefail

SERVICE_NAME="kelicloud-agent-rs"
DEFAULT_SERVICE_UNIT="kelicloud-agent-rs.service"
BIN_PATH="/usr/local/bin/kelicloud-agent-rs"
CONFIG_DIR="/etc/kelicloud-agent-rs"
CONFIG_FILE="${CONFIG_DIR}/config.env"
SERVICE_FILE="/etc/systemd/system/${SERVICE_NAME}.service"
REPO="keli-123456/kelicloud-agent-rs"

ENDPOINT=""
TOKEN=""
AUTO_DISCOVERY_KEY=""
SOURCE_BINARY=""
VERSION="latest"
GITHUB_PROXY=""
DISABLE_WEB_SSH="false"
INSECURE=""
INTERVAL=""
MAX_RETRIES=""
RECONNECT_INTERVAL=""
INFO_REPORT_INTERVAL=""
CF_ACCESS_CLIENT_ID=""
CF_ACCESS_CLIENT_SECRET=""
CUSTOM_DNS=""
MEMORY_INCLUDE_CACHE=""
INCLUDE_NICS=""
EXCLUDE_NICS=""
INCLUDE_MOUNTPOINTS=""
MONTH_ROTATE=""
KEEP_CONFIG="false"
COMMAND=""

usage() {
    cat <<'EOF'
kelicloud-agent-rs Linux installer

Usage:
  install.sh -e URL (--token TOKEN | --auto-discovery KEY) [options]
  install.sh install --endpoint URL --token TOKEN [options]
  install.sh uninstall [--keep-config]
  install.sh restart
  install.sh status
  install.sh render-service [--bin PATH] [--env PATH]
  install.sh render-env --endpoint URL --token TOKEN [options]

Install options:
  -e, --endpoint URL             Backend endpoint, for AGENT_ENDPOINT
  -t, --token TOKEN              Client token, for AGENT_TOKEN
  --auto-discovery KEY           Auto-discovery key, for AGENT_AUTO_DISCOVERY_KEY
  --source-binary PATH           Install an already built local binary
  --version VERSION              GitHub release version to download, default latest
  --install-version VERSION      Alias of --version
  --github-proxy URL             Prefix for GitHub download URL
  --install-ghproxy URL          Alias of --github-proxy
  --install-dir PATH             Install binary and config under PATH
  --bin PATH                     Binary install path, default /usr/local/bin/kelicloud-agent-rs
  --env PATH                     Environment file path, default /etc/kelicloud-agent-rs/config.env
  --disable-web-ssh              Set AGENT_DISABLE_WEB_SSH=true
  --insecure                     Set AGENT_INSECURE=true
  --ignore-unsafe-cert           Alias of --insecure
  --interval SECONDS             Set AGENT_INTERVAL
  --max-retries COUNT            Set AGENT_MAX_RETRIES
  --reconnect-interval SECONDS   Set AGENT_RECONNECT_INTERVAL
  --info-report-interval MINS    Set AGENT_INFO_REPORT_INTERVAL
  --cf-access-client-id ID       Set AGENT_CF_ACCESS_CLIENT_ID
  --cf-access-client-secret SEC  Set AGENT_CF_ACCESS_CLIENT_SECRET
  --custom-dns SERVER            Set AGENT_CUSTOM_DNS
  --memory-include-cache         Set AGENT_MEMORY_INCLUDE_CACHE=true
  --include-nics CSV             Set AGENT_INCLUDE_NICS
  --exclude-nics CSV             Set AGENT_EXCLUDE_NICS
  --include-mountpoint LIST      Set AGENT_INCLUDE_MOUNTPOINTS
  --include-mountpoints LIST     Alias of --include-mountpoint
  --month-rotate DAY             Set AGENT_MONTH_ROTATE
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

shell_quote() {
    printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}

emit_env() {
    local key="$1"
    local value="$2"
    if [[ -n "$value" ]]; then
        printf '%s=' "$key"
        shell_quote "$value"
        printf '\n'
    fi
}

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            install|uninstall|restart|status|render-service|render-env)
                if [[ -n "$COMMAND" ]]; then
                    die "multiple commands specified: ${COMMAND} and $1"
                fi
                COMMAND="$1"
                shift
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
                CONFIG_DIR="$2"
                BIN_PATH="${CONFIG_DIR%/}/kelicloud-agent-rs"
                CONFIG_FILE="${CONFIG_DIR%/}/config.env"
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

require_linux_systemd() {
    [[ "$(uname -s)" == "Linux" ]] || die "this installer supports Linux only"
    command -v systemctl >/dev/null 2>&1 || die "systemctl is required"
}

require_root() {
    [[ "${EUID:-$(id -u)}" -eq 0 ]] || die "please run as root"
}

stop_existing_service_for_upgrade() {
    if systemctl list-unit-files "${SERVICE_NAME}.service" >/dev/null 2>&1 &&
        systemctl list-unit-files "${SERVICE_NAME}.service" | grep -q "${SERVICE_NAME}.service"; then
        systemctl stop "${SERVICE_NAME}.service" >/dev/null 2>&1 || true
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

download_url() {
    local arch="$1"
    local version_path="latest/download"
    if [[ "$VERSION" != "latest" ]]; then
        version_path="download/${VERSION}"
    fi
    local url="https://github.com/${REPO}/releases/${version_path}/kelicloud-agent-rs-linux-${arch}"
    if [[ -n "$GITHUB_PROXY" ]]; then
        printf '%s/%s' "$GITHUB_PROXY" "$url"
    else
        printf '%s' "$url"
    fi
}

install_binary() {
    mkdir -p "$(dirname "$BIN_PATH")"
    if [[ -n "$SOURCE_BINARY" ]]; then
        [[ -f "$SOURCE_BINARY" ]] || die "source binary not found: $SOURCE_BINARY"
        install -m 0755 "$SOURCE_BINARY" "$BIN_PATH"
        return
    fi

    command -v curl >/dev/null 2>&1 || die "curl is required when --source-binary is not used"
    local arch
    arch="$(detect_arch)"
    local url
    url="$(download_url "$arch")"
    log "Downloading ${url}"
    curl -fL "$url" -o "$BIN_PATH"
    chmod 0755 "$BIN_PATH"
}

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

render_service() {
    cat <<EOF
[Unit]
Description=kelicloud Agent RS
Wants=network-online.target
After=network-online.target

[Service]
Type=simple
EnvironmentFile=${CONFIG_FILE}
ExecStart=${BIN_PATH}
Restart=always
RestartSec=5
User=root
LimitNOFILE=1048576

[Install]
WantedBy=multi-user.target
EOF
}

write_config() {
    [[ -n "$ENDPOINT" ]] || die "--endpoint is required"
    if [[ -z "$TOKEN" && -z "$AUTO_DISCOVERY_KEY" ]]; then
        die "--token or --auto-discovery is required"
    fi
    mkdir -p "$CONFIG_DIR"
    render_env > "$CONFIG_FILE"
    chmod 0600 "$CONFIG_FILE"
}

write_service() {
    render_service > "$SERVICE_FILE"
    chmod 0644 "$SERVICE_FILE"
    systemctl daemon-reload
    systemctl enable --now "${SERVICE_NAME}.service"
}

install_agent() {
    require_linux_systemd
    require_root
    stop_existing_service_for_upgrade
    install_binary
    write_config
    write_service
    log "Installed ${SERVICE_NAME}"
    log "Config: ${CONFIG_FILE}"
    log "Service: ${SERVICE_FILE}"
}

uninstall_agent() {
    require_linux_systemd
    require_root
    systemctl stop "${SERVICE_NAME}.service" >/dev/null 2>&1 || true
    systemctl disable "${SERVICE_NAME}.service" >/dev/null 2>&1 || true
    rm -f "$SERVICE_FILE"
    rm -f "$BIN_PATH"
    if [[ "$KEEP_CONFIG" != "true" ]]; then
        rm -f "$CONFIG_FILE"
        rm -f "$(dirname "$BIN_PATH")/auto-discovery.json"
        rm -f "${CONFIG_DIR}/auto-discovery.json"
        rmdir "$CONFIG_DIR" >/dev/null 2>&1 || true
    fi
    systemctl daemon-reload
    log "Uninstalled ${SERVICE_NAME}"
}

restart_agent() {
    require_linux_systemd
    require_root
    systemctl restart "${SERVICE_NAME}.service"
}

status_agent() {
    require_linux_systemd
    systemctl status "${SERVICE_NAME}.service"
}

main() {
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

main "$@"
