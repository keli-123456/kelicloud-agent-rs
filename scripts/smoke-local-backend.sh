#!/usr/bin/env bash
set -Eeuo pipefail

KELICLOUD_BACKEND_REPO="${KELICLOUD_BACKEND_REPO:-https://github.com/keli-123456/kelicloud.git}"
KELICLOUD_BACKEND_REF="${KELICLOUD_BACKEND_REF:-main}"
KELICLOUD_BACKEND_PATH="${KELICLOUD_BACKEND_PATH:-}"
KELICLOUD_PREPARE_FRONTEND="${KELICLOUD_PREPARE_FRONTEND:-true}"
KOMARI_FRONTEND_REF="${KOMARI_FRONTEND_REF:-main}"
KELICLOUD_SMOKE_KTP_TCP="${KELICLOUD_SMOKE_KTP_TCP:-false}"
KELICLOUD_SMOKE_TUNNEL_DATA_SCHEME="${KELICLOUD_SMOKE_TUNNEL_DATA_SCHEME:-}"
if [[ -z "${KELICLOUD_SMOKE_TUNNEL_DATA_SCHEME}" ]]; then
    if [[ "${KELICLOUD_SMOKE_KTP_TCP}" == "true" ]]; then
        KELICLOUD_SMOKE_TUNNEL_DATA_SCHEME="ktp+tcp"
    else
        KELICLOUD_SMOKE_TUNNEL_DATA_SCHEME="websocket"
    fi
fi

BACKEND_LISTEN="${BACKEND_LISTEN:-127.0.0.1:25775}"
BACKEND_ENDPOINT="${BACKEND_ENDPOINT:-http://${BACKEND_LISTEN}}"
BACKEND_START_TIMEOUT_SECONDS="${BACKEND_START_TIMEOUT_SECONDS:-240}"
KTP_TCP_LISTEN="${KTP_TCP_LISTEN:-}"
KTP_TLS_CERT_FILE="${KTP_TLS_CERT_FILE:-}"
KTP_TLS_KEY_FILE="${KTP_TLS_KEY_FILE:-}"
KTP_TLS_CA_CERT="${KTP_TLS_CA_CERT:-}"
KTP_TLS_SERVER_NAME="${KTP_TLS_SERVER_NAME:-localhost}"
AGENT_TUNNEL_KTP_TCP_AUTH_VERSION="${AGENT_TUNNEL_KTP_TCP_AUTH_VERSION:-v1}"
KTP_DIAGNOSTICS_TIMEOUT_SECONDS="${KTP_DIAGNOSTICS_TIMEOUT_SECONDS:-45}"
KTP_LIVE_CANARY_MIN_LINES="${KTP_LIVE_CANARY_MIN_LINES:-1}"
KELICLOUD_TUNNEL_ECHO_ROUNDS="${KELICLOUD_TUNNEL_ECHO_ROUNDS:-1}"
KELICLOUD_TUNNEL_ECHO_CLIENTS="${KELICLOUD_TUNNEL_ECHO_CLIENTS:-1}"
KELICLOUD_TUNNEL_MAX_CONCURRENT_SESSIONS="${KELICLOUD_TUNNEL_MAX_CONCURRENT_SESSIONS:-}"
KELICLOUD_TUNNEL_ECHO_PROFILE="${KELICLOUD_TUNNEL_ECHO_PROFILE:-fixed}"
KELICLOUD_TUNNEL_ECHO_PAYLOAD_BYTES="${KELICLOUD_TUNNEL_ECHO_PAYLOAD_BYTES:-0}"
KELICLOUD_TUNNEL_ECHO_EVIDENCE="${KELICLOUD_TUNNEL_ECHO_EVIDENCE:-}"
ADMIN_USERNAME="${ADMIN_USERNAME:-admin}"
ADMIN_PASSWORD="${ADMIN_PASSWORD:-admin-smoke-password}"

KOMARI_DB_HOST="${KOMARI_DB_HOST:-127.0.0.1}"
KOMARI_DB_PORT="${KOMARI_DB_PORT:-3306}"
KOMARI_DB_USER="${KOMARI_DB_USER:-root}"
KOMARI_DB_PASS="${KOMARI_DB_PASS:-rootpass}"
KOMARI_DB_NAME="${KOMARI_DB_NAME:-komari}"

SMOKE_LOG_DIR="${SMOKE_LOG_DIR:-smoke-logs}"
SMOKE_WORK_DIR="${SMOKE_WORK_DIR:-}"
AGENT_PID=""
BACKEND_PID=""
ECHO_PID=""
BACKEND_DIR=""
WORK_DIR=""
COOKIE_JAR=""
BACKEND_LOG=""
AGENT_LOG=""
HELPER_LOG=""
TUNNEL_ECHO_LOG=""
TUNNEL_ECHO_EVIDENCE_FILE=""
AUTO_DISCOVERY_KEY=""
SMOKE_AGENT_HOSTNAME="${SMOKE_AGENT_HOSTNAME:-agent-rs-smoke}"
SMOKE_AGENT_CLIENT_NAME="Auto-${SMOKE_AGENT_HOSTNAME}"
SMOKE_TUNNEL_GROUP="${SMOKE_TUNNEL_GROUP:-agent-rs-smoke}"
ROTATED_AGENT_TOKEN=""
TUNNEL_TARGET_PORT=""
TUNNEL_LISTEN_PORT=""
TUNNEL_RULE_ID=""
KTP_EVIDENCE_FILE=""
CURRENT_STAGE="startup"

log() {
    printf '%s\n' "$*"
}

github_escape() {
    local value="$1"
    value="${value//'%'/'%25'}"
    value="${value//$'\r'/'%0D'}"
    value="${value//$'\n'/'%0A'}"
    printf '%s' "${value}"
}

emit_error() {
    local message="$1"
    if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
        printf '::error title=Local backend smoke::%s\n' "$(github_escape "${message}")"
    fi
    printf 'error: %s\n' "${message}" >&2
}

die() {
    emit_error "$*"
    exit 1
}

set_stage() {
    CURRENT_STAGE="$1"
    log "==> ${CURRENT_STAGE}"
    if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
        printf '::notice title=Local backend smoke::%s\n' "$(github_escape "${CURRENT_STAGE}")"
    fi
}

log_tail_for_error() {
    local output=""
    local file
    for file in "${AGENT_LOG}" "${HELPER_LOG}" "${BACKEND_LOG}"; do
        if [[ -n "${file}" && -f "${file}" ]]; then
            output+=$'\n\n'
            output+="--- ${file} tail ---"
            output+=$'\n'
            output+="$(tail -n 40 "${file}" 2>/dev/null || true)"
        fi
    done
    printf '%s' "${output}"
}

on_error() {
    local status="$?"
    trap - ERR
    emit_error "failed during ${CURRENT_STAGE} (exit ${status})$(log_tail_for_error)"
    exit "${status}"
}
trap on_error ERR

repo_root() {
    local script_dir
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    cd "${script_dir}/.." && pwd
}

cleanup() {
    if [[ -n "${AGENT_PID}" ]] && kill -0 "${AGENT_PID}" >/dev/null 2>&1; then
        kill "${AGENT_PID}" >/dev/null 2>&1 || true
        wait "${AGENT_PID}" >/dev/null 2>&1 || true
    fi
    if [[ -n "${BACKEND_PID}" ]] && kill -0 "${BACKEND_PID}" >/dev/null 2>&1; then
        kill "${BACKEND_PID}" >/dev/null 2>&1 || true
        wait "${BACKEND_PID}" >/dev/null 2>&1 || true
    fi
    if [[ -n "${ECHO_PID}" ]] && kill -0 "${ECHO_PID}" >/dev/null 2>&1; then
        kill "${ECHO_PID}" >/dev/null 2>&1 || true
        wait "${ECHO_PID}" >/dev/null 2>&1 || true
    fi
    if [[ -n "${SMOKE_WORK_DIR}" && "${SMOKE_WORK_DIR}" == /tmp/* && -d "${SMOKE_WORK_DIR}" ]]; then
        rm -rf "${SMOKE_WORK_DIR}"
    fi
}
trap cleanup EXIT

require_command() {
    command -v "$1" >/dev/null 2>&1 || die "$1 command is required"
}

require_positive_integer() {
    local name="$1"
    local value="$2"
    [[ "${value}" =~ ^[1-9][0-9]*$ ]] || die "${name} must be a positive integer"
}

require_non_negative_integer() {
    local name="$1"
    local value="$2"
    [[ "${value}" =~ ^[0-9]+$ ]] || die "${name} must be a non-negative integer"
}

require_tunnel_echo_profile() {
    case "${KELICLOUD_TUNNEL_ECHO_PROFILE}" in
        "fixed" | "rdp-like")
            ;;
        *)
            die "KELICLOUD_TUNNEL_ECHO_PROFILE must be fixed or rdp-like"
            ;;
    esac
}

ktp_tcp_smoke_enabled() {
    [[ "${KELICLOUD_SMOKE_TUNNEL_DATA_SCHEME}" == "ktp+tcp" || "${KELICLOUD_SMOKE_TUNNEL_DATA_SCHEME}" == "ktp+tls" ]]
}

ktp_plain_tcp_smoke_enabled() {
    [[ "${KELICLOUD_SMOKE_TUNNEL_DATA_SCHEME}" == "ktp+tcp" ]]
}

ktp_tls_smoke_enabled() {
    [[ "${KELICLOUD_SMOKE_TUNNEL_DATA_SCHEME}" == "ktp+tls" ]]
}

ktp_live_canary_carrier() {
    if ktp_tls_smoke_enabled; then
        printf 'ktp_tls'
    else
        printf 'ktp_tcp'
    fi
}

ktp_backend_bool() {
    if "$@"; then
        printf 'true'
    else
        printf 'false'
    fi
}

ktp_agent_tunnel_address() {
    if ktp_tls_smoke_enabled; then
        printf 'ktp+tls://%s?server_name=%s' "${KTP_TCP_LISTEN}" "${KTP_TLS_SERVER_NAME}"
    else
        printf '%s' "${KTP_TCP_LISTEN}"
    fi
}

json_value() {
    local path="$1"
    python3 -c '
import json
import sys

path = sys.argv[1].split(".")
try:
    data = json.load(sys.stdin)
    for key in path:
        if isinstance(data, list):
            data = data[int(key)]
        elif isinstance(data, dict):
            data = data.get(key)
        else:
            data = None
        if data is None:
            break
    if data is None:
        print("")
    elif isinstance(data, (dict, list)):
        print(json.dumps(data, separators=(",", ":")))
    else:
        print(data)
except Exception:
    print("")
' "$path"
}

json_payload() {
    python3 - "$@" <<'PY'
import json
import sys

kind = sys.argv[1]
if kind == "exec":
    print(json.dumps({"command": sys.argv[2], "clients": [sys.argv[3]]}))
elif kind == "ping":
    print(json.dumps({
        "name": "agent-rs-smoke",
        "target": sys.argv[2],
        "type": "tcp",
        "interval": 1,
        "clients": [sys.argv[3]],
    }))
elif kind == "cn":
    print(json.dumps({
        "cn_connectivity_enabled": True,
        "cn_connectivity_target": "127.0.0.1",
        "cn_connectivity_interval": 1,
        "cn_connectivity_retry_attempts": 1,
        "cn_connectivity_retry_delay_seconds": 1,
        "cn_connectivity_timeout_seconds": 1,
    }))
elif kind == "client-token":
    print(json.dumps({"token": sys.argv[2]}))
elif kind == "client-group":
    print(json.dumps({"group": sys.argv[2]}))
elif kind == "tunnel-rule":
    print(json.dumps({
        "name": "agent-rs-smoke-tunnel",
        "enabled": True,
        "protocol": "tcp",
        "ingress_group": sys.argv[2],
        "listen_address": "127.0.0.1",
        "listen_port": int(sys.argv[3]),
        "egress_group": sys.argv[2],
        "target_host": "127.0.0.1",
        "target_port": int(sys.argv[4]),
        "source_allowlist": "127.0.0.1/32",
        "max_concurrent_sessions": int(sys.argv[5]),
        "remark": "local backend smoke tunnel relay",
    }))
else:
    raise SystemExit(f"unknown payload kind: {kind}")
PY
}

tunnel_rule_max_concurrent_sessions() {
    local minimum=32
    if ((KELICLOUD_TUNNEL_ECHO_CLIENTS > minimum)); then
        minimum="${KELICLOUD_TUNNEL_ECHO_CLIENTS}"
    fi
    if [[ -z "${KELICLOUD_TUNNEL_MAX_CONCURRENT_SESSIONS}" ]]; then
        printf '%s\n' "${minimum}"
        return
    fi
    require_positive_integer "KELICLOUD_TUNNEL_MAX_CONCURRENT_SESSIONS" "${KELICLOUD_TUNNEL_MAX_CONCURRENT_SESSIONS}"
    if ((KELICLOUD_TUNNEL_MAX_CONCURRENT_SESSIONS < minimum)); then
        die "KELICLOUD_TUNNEL_MAX_CONCURRENT_SESSIONS must be at least ${minimum}"
    fi
    printf '%s\n' "${KELICLOUD_TUNNEL_MAX_CONCURRENT_SESSIONS}"
}

pick_free_tcp_port() {
    python3 -c 'import socket
sock = socket.socket()
sock.bind(("127.0.0.1", 0))
print(sock.getsockname()[1])
sock.close()'
}

configure_ktp_tcp_smoke() {
    case "${KELICLOUD_SMOKE_TUNNEL_DATA_SCHEME}" in
        websocket | ktp+tcp | ktp+tls)
            ;;
        *)
            die "KELICLOUD_SMOKE_TUNNEL_DATA_SCHEME must be websocket, ktp+tcp, or ktp+tls"
            ;;
    esac
    if ! ktp_tcp_smoke_enabled; then
        return
    fi
    if [[ -z "${KTP_TCP_LISTEN}" ]]; then
        KTP_TCP_LISTEN="127.0.0.1:$(pick_free_tcp_port)"
    fi
    if ktp_tls_smoke_enabled; then
        configure_ktp_tls_certificates
        log "KTP TLS tunnel data smoke enabled at ${KTP_TCP_LISTEN} server_name=${KTP_TLS_SERVER_NAME}"
    else
        log "KTP TCP tunnel data smoke enabled at ${KTP_TCP_LISTEN}"
    fi
}

configure_ktp_tls_certificates() {
    require_command openssl

    local tls_dir="${WORK_DIR}/ktp-tls"
    mkdir -p "${tls_dir}"

    KTP_TLS_CA_CERT="${KTP_TLS_CA_CERT:-${tls_dir}/ktp-ca.pem}"
    KTP_TLS_CERT_FILE="${KTP_TLS_CERT_FILE:-${tls_dir}/ktp-server.pem}"
    KTP_TLS_KEY_FILE="${KTP_TLS_KEY_FILE:-${tls_dir}/ktp-server.key}"

    if [[ -s "${KTP_TLS_CA_CERT}" && -s "${KTP_TLS_CERT_FILE}" && -s "${KTP_TLS_KEY_FILE}" ]]; then
        return
    fi

    local ca_key="${tls_dir}/ktp-ca.key"
    local csr_file="${tls_dir}/ktp-server.csr"
    local ext_file="${tls_dir}/ktp-server.ext"
    local serial_file="${tls_dir}/ktp-ca.srl"

    cat >"${ext_file}" <<EOF
subjectAltName=DNS:${KTP_TLS_SERVER_NAME},IP:127.0.0.1
extendedKeyUsage=serverAuth
basicConstraints=CA:FALSE
keyUsage=digitalSignature,keyEncipherment
EOF

    openssl genrsa -out "${ca_key}" 2048 >/dev/null 2>&1
    openssl req -x509 -new -nodes -key "${ca_key}" -sha256 -days 1 \
        -subj "/CN=kelicloud ktp smoke ca" \
        -out "${KTP_TLS_CA_CERT}" >/dev/null 2>&1
    openssl genrsa -out "${KTP_TLS_KEY_FILE}" 2048 >/dev/null 2>&1
    openssl req -new -key "${KTP_TLS_KEY_FILE}" \
        -subj "/CN=${KTP_TLS_SERVER_NAME}" \
        -out "${csr_file}" >/dev/null 2>&1
    rm -f "${serial_file}"
    openssl x509 -req -in "${csr_file}" \
        -CA "${KTP_TLS_CA_CERT}" \
        -CAkey "${ca_key}" \
        -CAcreateserial \
        -out "${KTP_TLS_CERT_FILE}" \
        -days 1 \
        -sha256 \
        -extfile "${ext_file}" >/dev/null 2>&1
}

wait_for_http() {
    local url="$1"
    local timeout_seconds="$2"
    local deadline=$((SECONDS + timeout_seconds))
    until curl -fsS "${url}" >/dev/null 2>&1; do
        if (( SECONDS >= deadline )); then
            die "timed out waiting for ${url}$(log_tail_for_error)"
        fi
        sleep 1
    done
}

wait_for_log() {
    local file="$1"
    local needle="$2"
    local timeout_seconds="$3"
    local deadline=$((SECONDS + timeout_seconds))
    until [[ -f "${file}" ]] && grep -Fq "${needle}" "${file}"; do
        if (( SECONDS >= deadline )); then
            if [[ -f "${file}" ]]; then
                tail -n 120 "${file}" >&2 || true
            fi
            die "timed out waiting for log text: ${needle}"
        fi
        sleep 1
    done
}

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

curl_api() {
    local method="$1"
    local path="$2"
    local payload="${3:-}"
    if [[ -n "${payload}" ]]; then
        curl -fsS -b "${COOKIE_JAR}" -c "${COOKIE_JAR}" \
            -H "Content-Type: application/json" \
            -X "${method}" \
            --data "${payload}" \
            "${BACKEND_ENDPOINT}${path}"
    else
        curl -fsS -b "${COOKIE_JAR}" -c "${COOKIE_JAR}" \
            -X "${method}" \
            "${BACKEND_ENDPOINT}${path}"
    fi
}

wait_for_mysql() {
    if ! command -v mysqladmin >/dev/null 2>&1; then
        log "mysqladmin not found; skipping active MySQL wait"
        return
    fi

    local deadline=$((SECONDS + 90))
    until mysqladmin ping \
        -h"${KOMARI_DB_HOST}" \
        -P"${KOMARI_DB_PORT}" \
        -u"${KOMARI_DB_USER}" \
        -p"${KOMARI_DB_PASS}" \
        --silent >/dev/null 2>&1; do
        if (( SECONDS >= deadline )); then
            die "timed out waiting for MySQL"
        fi
        sleep 2
    done

    if command -v mysql >/dev/null 2>&1; then
        mysql \
            -h"${KOMARI_DB_HOST}" \
            -P"${KOMARI_DB_PORT}" \
            -u"${KOMARI_DB_USER}" \
            -p"${KOMARI_DB_PASS}" \
            -e "CREATE DATABASE IF NOT EXISTS \`${KOMARI_DB_NAME}\`"
    fi
}

ensure_backend_frontend_placeholder() {
    local index_file="${BACKEND_DIR}/public/frontend/dist/index.html"
    if [[ -f "${index_file}" ]]; then
        return
    fi

    log "Creating minimal frontend placeholder for headless backend build"
    mkdir -p "$(dirname "${index_file}")"
    cat >"${index_file}" <<'HTML'
<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <title>Komari Monitor</title>
</head>
<body>
  <div id="root">A simple server monitor tool.</div>
</body>
</html>
HTML
}

prepare_backend() {
    if [[ -n "${KELICLOUD_BACKEND_PATH}" ]]; then
        BACKEND_DIR="$(cd "${KELICLOUD_BACKEND_PATH}" && pwd)"
        log "Using local backend checkout ${BACKEND_DIR}"
    else
        BACKEND_DIR="${WORK_DIR}/kelicloud"
        log "Cloning backend ${KELICLOUD_BACKEND_REPO} @ ${KELICLOUD_BACKEND_REF}"
        git clone "${KELICLOUD_BACKEND_REPO}" "${BACKEND_DIR}"
        git -C "${BACKEND_DIR}" checkout "${KELICLOUD_BACKEND_REF}"
    fi

    if [[ "${KELICLOUD_PREPARE_FRONTEND}" == "true" ]]; then
        log "Preparing frontend bundle with scripts/prepare-frontend.sh"
        (cd "${BACKEND_DIR}" && KOMARI_FRONTEND_REF="${KOMARI_FRONTEND_REF}" bash scripts/prepare-frontend.sh)
    else
        ensure_backend_frontend_placeholder
    fi

    log "Building kelicloud backend"
    (cd "${BACKEND_DIR}" && go build -o "${WORK_DIR}/kelicloud-backend" .)
}

start_backend() {
    BACKEND_LOG="${SMOKE_LOG_DIR}/backend.log"
    log "Starting backend at ${BACKEND_ENDPOINT}"
    local ktp_tcp_enabled ktp_tls_enabled
    ktp_tcp_enabled="$(ktp_backend_bool ktp_plain_tcp_smoke_enabled)"
    ktp_tls_enabled="$(ktp_backend_bool ktp_tls_smoke_enabled)"
    (
        cd "${BACKEND_DIR}"
        env \
            ADMIN_USERNAME="${ADMIN_USERNAME}" \
            ADMIN_PASSWORD="${ADMIN_PASSWORD}" \
            KOMARI_LISTEN="${BACKEND_LISTEN}" \
            KOMARI_DB_HOST="${KOMARI_DB_HOST}" \
            KOMARI_DB_PORT="${KOMARI_DB_PORT}" \
            KOMARI_DB_USER="${KOMARI_DB_USER}" \
            KOMARI_DB_PASS="${KOMARI_DB_PASS}" \
            KOMARI_DB_NAME="${KOMARI_DB_NAME}" \
            KOMARI_TUNNEL_KTP_TCP_ENABLED="${ktp_tcp_enabled}" \
            KOMARI_TUNNEL_KTP_TCP_LISTEN="${KTP_TCP_LISTEN}" \
            KOMARI_TUNNEL_KTP_TCP_ADDRESS="$(ktp_agent_tunnel_address)" \
            KOMARI_TUNNEL_KTP_TLS_ENABLED="${ktp_tls_enabled}" \
            KOMARI_TUNNEL_KTP_TLS_LISTEN="${KTP_TCP_LISTEN}" \
            KOMARI_TUNNEL_KTP_TLS_CERT_FILE="${KTP_TLS_CERT_FILE}" \
            KOMARI_TUNNEL_KTP_TLS_KEY_FILE="${KTP_TLS_KEY_FILE}" \
            KOMARI_SECURITY_HSTS="false" \
            "${WORK_DIR}/kelicloud-backend" server
    ) >"${BACKEND_LOG}" 2>&1 &
    BACKEND_PID="$!"
    wait_for_http "${BACKEND_ENDPOINT}/ping" "${BACKEND_START_TIMEOUT_SECONDS}"
}

login_admin() {
    COOKIE_JAR="${WORK_DIR}/cookies.txt"
    local login_payload
    login_payload="$(python3 -c '
import json
import sys
print(json.dumps({"username": sys.argv[1], "password": sys.argv[2]}))
' "${ADMIN_USERNAME}" "${ADMIN_PASSWORD}")"

    local response deadline
    deadline=$((SECONDS + 90))
    until response="$(curl -fsS -c "${COOKIE_JAR}" \
        -H "Content-Type: application/json" \
        --data "${login_payload}" \
        "${BACKEND_ENDPOINT}/api/login" 2>/dev/null)" &&
        SESSION_TOKEN="$(printf '%s' "${response}" | json_value "data.set-cookie.session_token")" &&
        [[ -n "${SESSION_TOKEN}" ]]; do
        if (( SECONDS >= deadline )); then
            die "timed out waiting for admin login"
        fi
        sleep 1
    done
}

load_auto_discovery_key() {
    local response
    response="$(curl_api GET "/api/admin/settings/")"
    AUTO_DISCOVERY_KEY="$(printf '%s' "${response}" | json_value "data.auto_discovery_key")"
    [[ -n "${AUTO_DISCOVERY_KEY}" ]] || die "settings response did not include auto_discovery_key"
    log "Loaded auto-discovery key for smoke"
}

resolve_auto_discovery_client() {
    local response uuid deadline
    deadline=$((SECONDS + 45))
    until response="$(curl_api GET "/api/admin/client/list" 2>/dev/null)" &&
        uuid="$(printf '%s' "${response}" | python3 -c 'import json, sys
target = sys.argv[1]
try:
    data = json.load(sys.stdin)
except Exception:
    print("")
    raise SystemExit(0)
clients = data.get("data", data) if isinstance(data, dict) else data
if not isinstance(clients, list):
    print("")
    raise SystemExit(0)
for client in reversed(clients):
    if isinstance(client, dict) and client.get("name") == target:
        print(client.get("uuid", ""))
        break
else:
    print("")' "${SMOKE_AGENT_CLIENT_NAME}")" && [[ -n "${uuid}" ]]; do
        if (( SECONDS >= deadline )); then
            die "timed out waiting for auto-discovered client ${SMOKE_AGENT_CLIENT_NAME}"
        fi
        sleep 1
    done

    bind_auto_discovery_client "${uuid}"
}

bind_auto_discovery_client() {
    local uuid="$1"
    local response token
    [[ -n "${uuid}" ]] || die "auto-discovered client uuid is empty"
    CLIENT_UUID="${uuid}"
    response="$(curl_api GET "/api/admin/client/${CLIENT_UUID}/token")"
    token="$(printf '%s' "${response}" | json_value "token")"
    [[ -n "${token}" ]] || die "client token response did not include token"
    AGENT_TOKEN="${token}"
    log "Resolved auto-discovered smoke client ${CLIENT_UUID}"
}

latest_auto_discovery_registered_uuid() {
    local uuid
    uuid="$(grep -F "smoke: auto_discovery_registered" "${AGENT_LOG}" |
        tail -n 1 |
        sed -n 's/.*[[:space:]]uuid=\([^[:space:]]*\).*/\1/p')"
    [[ -n "${uuid}" ]] || die "agent log did not include an auto-discovery uuid"
    printf '%s\n' "${uuid}"
}

start_agent() {
    local root="$1"
    local tunnel_args=()
    AGENT_LOG="${SMOKE_LOG_DIR}/agent.log"
    : >"${AGENT_LOG}"
    if ktp_tcp_smoke_enabled; then
        tunnel_args+=(--tunnel-ktp-tcp-address "$(ktp_agent_tunnel_address)")
        tunnel_args+=(--tunnel-ktp-tcp-auth-version "${AGENT_TUNNEL_KTP_TCP_AUTH_VERSION}")
        if ktp_tls_smoke_enabled; then
            tunnel_args+=(--tunnel-ktp-tls-ca-cert "${KTP_TLS_CA_CERT}")
        fi
    fi

    log "Building agent and smoke helpers"
    (cd "${root}" && cargo build --locked --release --bin kelicloud-agent-rs --bin admin-terminal-smoke --bin smoke-summary)
    rm -f "${root}/target/release/auto-discovery.json"

    log "Starting kelicloud-agent-rs"
    AGENT_TUNNEL_DATA_ENABLED=true HOSTNAME="${SMOKE_AGENT_HOSTNAME}" "${root}/target/release/kelicloud-agent-rs" \
        --endpoint "${BACKEND_ENDPOINT}" \
        --auto-discovery "${AUTO_DISCOVERY_KEY}" \
        --interval 1 \
        --max-retries 3 \
        --reconnect-interval 1 \
        --info-report-interval 0 \
        "${tunnel_args[@]}" >>"${AGENT_LOG}" 2>&1 &
    AGENT_PID="$!"

    wait_for_log "${AGENT_LOG}" "smoke: report_websocket_connected" 45
    wait_for_log "${AGENT_LOG}" "smoke: report_sent" 45
}

rotate_auto_discovery_token() {
    ROTATED_AGENT_TOKEN="rotated-${CLIENT_UUID}-${SECONDS}"
    local payload
    payload="$(json_payload client-token "${ROTATED_AGENT_TOKEN}")"
    curl_api POST "/api/admin/client/${CLIENT_UUID}/edit" "${payload}" >/dev/null
    log "Rotated auto-discovered client token through admin API"
}

wait_for_auto_discovery_recovery() {
    wait_for_log_count "${AGENT_LOG}" "smoke: token_recovered" 1 120
    wait_for_log_count "${AGENT_LOG}" "smoke: auto_discovery_registered" 2 120
    wait_for_log_count "${AGENT_LOG}" "smoke: report_websocket_connected" 2 120
    wait_for_log_count "${AGENT_LOG}" "smoke: report_sent" 2 120
    bind_auto_discovery_client "$(latest_auto_discovery_registered_uuid)"
}

stop_agent_process() {
    if [[ -n "${AGENT_PID}" ]] && kill -0 "${AGENT_PID}" >/dev/null 2>&1; then
        kill "${AGENT_PID}" >/dev/null 2>&1 || true
        wait "${AGENT_PID}" >/dev/null 2>&1 || true
    fi
    AGENT_PID=""
}

restart_agent_after_token_recovery() {
    local root="$1"
    local connected_count sent_count tunnel_args=()
    connected_count="$({ grep -F "smoke: report_websocket_connected" "${AGENT_LOG}" || true; } | wc -l | tr -d '[:space:]')"
    sent_count="$({ grep -F "smoke: report_sent" "${AGENT_LOG}" || true; } | wc -l | tr -d '[:space:]')"
    if ktp_tcp_smoke_enabled; then
        tunnel_args+=(--tunnel-ktp-tcp-address "$(ktp_agent_tunnel_address)")
        tunnel_args+=(--tunnel-ktp-tcp-auth-version "${AGENT_TUNNEL_KTP_TCP_AUTH_VERSION}")
        if ktp_tls_smoke_enabled; then
            tunnel_args+=(--tunnel-ktp-tls-ca-cert "${KTP_TLS_CA_CERT}")
        fi
    fi

    stop_agent_process
    printf '%s\n' "smoke: restarting_agent_after_token_recovery" >>"${AGENT_LOG}"
    log "Restarting kelicloud-agent-rs after token recovery so tunnel sockets use the recovered token"
    AGENT_TUNNEL_DATA_ENABLED=true HOSTNAME="${SMOKE_AGENT_HOSTNAME}" "${root}/target/release/kelicloud-agent-rs" \
        --endpoint "${BACKEND_ENDPOINT}" \
        --auto-discovery "${AUTO_DISCOVERY_KEY}" \
        --interval 1 \
        --max-retries 3 \
        --reconnect-interval 1 \
        --info-report-interval 0 \
        "${tunnel_args[@]}" >>"${AGENT_LOG}" 2>&1 &
    AGENT_PID="$!"

    wait_for_log_count "${AGENT_LOG}" "smoke: report_websocket_connected" "$((connected_count + 1))" 90
    wait_for_log_count "${AGENT_LOG}" "smoke: report_sent" "$((sent_count + 1))" 90
}

set_client_tunnel_group() {
    local payload
    payload="$(json_payload client-group "${SMOKE_TUNNEL_GROUP}")"
    curl_api POST "/api/admin/client/${CLIENT_UUID}/edit" "${payload}" >/dev/null
    log "Assigned smoke client ${CLIENT_UUID} to tunnel group ${SMOKE_TUNNEL_GROUP}"
}

enable_cn_connectivity_probe() {
    local payload
    payload="$(json_payload cn)"
    curl_api POST "/api/admin/settings/system" "${payload}" >/dev/null
    wait_for_log "${AGENT_LOG}" "smoke: cn_connectivity_config_received" 30
}

trigger_exec() {
    EXEC_MARK="kelicloud-agent-rs-exec-smoke"
    local payload response
    payload="$(json_payload exec "printf '${EXEC_MARK}\\n'" "${CLIENT_UUID}")"
    response="$(curl_api POST "/api/admin/task/exec" "${payload}")"
    EXEC_TASK_ID="$(printf '%s' "${response}" | json_value "data.task_id")"
    [[ -n "${EXEC_TASK_ID}" ]] || die "exec response did not include task_id"

    wait_for_log "${AGENT_LOG}" "smoke: task_result_uploaded" 45

    local result deadline
    deadline=$((SECONDS + 45))
    until result="$(curl_api GET "/api/admin/task/${EXEC_TASK_ID}/result/${CLIENT_UUID}" 2>/dev/null)" &&
        [[ "${result}" == *"${EXEC_MARK}"* && "${result}" == *'"exit_code":0'* ]]; do
        if (( SECONDS >= deadline )); then
            die "timed out waiting for exec API result"
        fi
        sleep 1
    done
}

trigger_ping() {
    local payload
    payload="$(json_payload ping "127.0.0.1:25775" "${CLIENT_UUID}")"
    curl_api POST "/api/admin/ping/add" "${payload}" >/dev/null
    wait_for_log "${AGENT_LOG}" "smoke: ping_result_uploaded" 45
}

trigger_terminal() {
    local root="$1"
    HELPER_LOG="${SMOKE_LOG_DIR}/admin-terminal-smoke.log"
    local mark="kelicloud-terminal-smoke"
    if ! "${root}/target/release/admin-terminal-smoke" \
        --endpoint "${BACKEND_ENDPOINT}" \
        --session-token "${SESSION_TOKEN}" \
        --client "${CLIENT_UUID}" \
        --command "printf '${mark}\\n'" \
        --expect "${mark}" \
        --timeout 90 >"${HELPER_LOG}" 2>&1; then
        local details=""
        if [[ -f "${HELPER_LOG}" ]]; then
            details="$(printf '\n--- %s tail ---\n' "${HELPER_LOG}")$(tail -n 80 "${HELPER_LOG}" 2>/dev/null || true)"
        fi
        die "admin-terminal-smoke failed${details}$(log_tail_for_error)"
    fi
    log "admin-terminal-smoke succeeded"
    if [[ -f "${HELPER_LOG}" ]]; then
        tail -n 20 "${HELPER_LOG}" || true
    fi
    if ! grep -Fq "smoke: terminal_session_started" "${AGENT_LOG}"; then
        die "admin-terminal-smoke succeeded but terminal_session_started was not observed$(log_tail_for_error)"
    fi
}

start_tunnel_echo_server() {
    TUNNEL_ECHO_LOG="${SMOKE_LOG_DIR}/tunnel-echo.log"
    local port_file="${WORK_DIR}/tunnel-echo-port"
    rm -f "${port_file}"
    python3 -u - "${port_file}" <<'PY' >"${TUNNEL_ECHO_LOG}" 2>&1 &
import socket
import sys

port_file = sys.argv[1]
server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
server.bind(("127.0.0.1", 0))
server.listen(16)
port = server.getsockname()[1]
with open(port_file, "w", encoding="utf-8") as fh:
    fh.write(str(port))
print(f"smoke tunnel echo listening port={port}", flush=True)
while True:
    conn, _addr = server.accept()
    with conn:
        data = conn.recv(65536)
        if data:
            conn.sendall(b"echo:" + data)
PY
    ECHO_PID="$!"
    wait_for_log "${TUNNEL_ECHO_LOG}" "smoke tunnel echo listening" 10
    TUNNEL_TARGET_PORT="$(cat "${port_file}")"
    [[ -n "${TUNNEL_TARGET_PORT}" ]] || die "tunnel echo server did not publish a port"
    log "Tunnel echo target is 127.0.0.1:${TUNNEL_TARGET_PORT}"
}

create_tunnel_rule() {
    TUNNEL_LISTEN_PORT="$(pick_free_tcp_port)"
    local payload response
    payload="$(json_payload tunnel-rule "${SMOKE_TUNNEL_GROUP}" "${TUNNEL_LISTEN_PORT}" "${TUNNEL_TARGET_PORT}" "$(tunnel_rule_max_concurrent_sessions)")"
    response="$(curl_api POST "/api/admin/tunnels" "${payload}")"
    TUNNEL_RULE_ID="$(printf '%s' "${response}" | json_value "data.id")"
    [[ -n "${TUNNEL_RULE_ID}" ]] || die "tunnel rule response did not include id"
    log "Created tunnel rule ${TUNNEL_RULE_ID}: 127.0.0.1:${TUNNEL_LISTEN_PORT} -> 127.0.0.1:${TUNNEL_TARGET_PORT}"
    wait_for_tunnel_rule_ready
}

wait_for_tunnel_rule_ready() {
    local response ready deadline
    deadline=$((SECONDS + 60))
    until response="$(curl_api GET "/api/admin/tunnels" 2>/dev/null)" &&
        ready="$(printf '%s' "${response}" | python3 -c '
import json
import sys

rule_id = int(sys.argv[1])
try:
    data = json.load(sys.stdin)
except Exception:
    print("invalid")
    raise SystemExit(0)
rules = data.get("data", {}).get("rules", []) if isinstance(data, dict) else []
for rule in rules:
    if isinstance(rule, dict) and int(rule.get("id", 0)) == rule_id:
        if rule.get("ingress_ready") and rule.get("egress_ready"):
            print("ready")
        else:
            print(str(rule.get("status", "not_ready")))
        break
else:
    print("missing")
' "${TUNNEL_RULE_ID}")" && [[ "${ready}" == "ready" ]]; do
        if (( SECONDS >= deadline )); then
            die "timed out waiting for tunnel rule ${TUNNEL_RULE_ID} to become ready (last status: ${ready:-unknown})$(log_tail_for_error)"
        fi
        sleep 1
    done
}

verify_tunnel_relay_echo() {
    local mark="kelicloud-tunnel-smoke-${TUNNEL_RULE_ID}"
    if [[ -n "${KELICLOUD_TUNNEL_ECHO_EVIDENCE}" ]]; then
        TUNNEL_ECHO_EVIDENCE_FILE="${KELICLOUD_TUNNEL_ECHO_EVIDENCE}"
    else
        TUNNEL_ECHO_EVIDENCE_FILE="${SMOKE_LOG_DIR}/tunnel-echo.evidence.md"
    fi

    if ! python3 - \
        "${TUNNEL_LISTEN_PORT}" \
        "${mark}" \
        "${KELICLOUD_TUNNEL_ECHO_ROUNDS}" \
        "${KELICLOUD_TUNNEL_ECHO_CLIENTS}" \
        "${KELICLOUD_TUNNEL_ECHO_PROFILE}" \
        "${KELICLOUD_TUNNEL_ECHO_PAYLOAD_BYTES}" \
        "${TUNNEL_ECHO_EVIDENCE_FILE}" <<'PY'
import math
import os
import socket
import sys
import threading
import time

port = int(sys.argv[1])
base_payload = sys.argv[2]
rounds = int(sys.argv[3])
client_count = int(sys.argv[4])
profile = sys.argv[5]
configured_payload_bytes = int(sys.argv[6])
evidence_file = sys.argv[7]
samples = []
errors = []
samples_lock = threading.Lock()

def fill_payload(prefix, target_bytes):
    base = prefix.encode("utf-8")
    if target_bytes <= 0 or len(base) >= target_bytes:
        return base
    filler = (b"-0123456789abcdef" * ((target_bytes // 17) + 2))
    return (base + filler)[:target_bytes]

def payload_for_round(client_id, round_no):
    if client_count == 1:
        payload_text = base_payload if rounds == 1 else f"{base_payload}-{round_no}"
    else:
        payload_text = f"{base_payload}-client-{client_id}-round-{round_no}"
    if profile == "fixed":
        return fill_payload(payload_text, configured_payload_bytes)

    max_payload_bytes = configured_payload_bytes if configured_payload_bytes > 0 else 8192
    rdp_like_sizes = [64, 96, 128, 256, 1024, max_payload_bytes]
    target_bytes = min(max_payload_bytes, rdp_like_sizes[(round_no - 1) % len(rdp_like_sizes)])
    return fill_payload(f"{payload_text}-rdp-like", target_bytes)

def percentile(sorted_values, percent):
    index = max(0, math.ceil((percent / 100.0) * len(sorted_values)) - 1)
    return sorted_values[min(index, len(sorted_values) - 1)]

def compact_text(value, max_len=240):
    text = str(value).replace("\n", "\\n").replace("|", "\\|")
    if len(text) <= max_len:
        return text
    return text[: max_len - 3] + "..."

def describe_response(response, expected):
    return (
        f"unexpected echo response len={len(response)} expected_len={len(expected)} "
        f"response_prefix={response[:128]!r} expected_prefix={expected[:128]!r}"
    )

def write_tunnel_echo_failure_evidence(detail):
    evidence_dir = os.path.dirname(evidence_file)
    if evidence_dir:
        os.makedirs(evidence_dir, exist_ok=True)
    with open(evidence_file, "w", encoding="utf-8") as fh:
        fh.write("# Tunnel Echo Evidence\n\n")
        fh.write("- status: failed\n")
        fh.write(f"- profile: {profile}\n")
        fh.write(f"- rounds: {rounds}\n")
        fh.write(f"- clients: {client_count}\n")
        fh.write(f"- expected_samples: {expected_samples}\n")
        fh.write(f"- collected_samples: {len(samples)}\n")
        fh.write(f"- echo_elapsed_micros: {echo_elapsed_micros}\n")
        fh.write(f"- failure: {compact_text(detail)}\n")
        if errors:
            fh.write("\n## Errors\n\n")
            for error in errors[:20]:
                fh.write(f"- {compact_text(error)}\n")
        if samples:
            fh.write("\n## Samples\n\n")
            fh.write("| client | round | payload_bytes | rtt_micros |\n")
            fh.write("| ---: | ---: | ---: | ---: |\n")
            for sample in sorted(samples, key=lambda item: (item["client"], item["round"])):
                fh.write(f"| {sample['client']} | {sample['round']} | {sample['payload_bytes']} | {sample['rtt_micros']} |\n")

def recv_expected_response(sock, expected_len):
    response = b""
    while len(response) < expected_len:
        chunk = sock.recv(min(65536, expected_len - len(response)))
        if not chunk:
            break
        response += chunk
    return response

def client_worker(client_id):
    for round in range(1, rounds + 1):
        payload = payload_for_round(client_id, round)
        expected = b"echo:" + payload
        deadline = time.time() + 45
        last_error = None
        while time.time() < deadline:
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=3) as sock:
                    sock.settimeout(5)
                    started = time.perf_counter()
                    sock.sendall(payload)
                    response = recv_expected_response(sock, len(expected))
                    if response == expected:
                        with samples_lock:
                            samples.append({
                                "client": client_id,
                                "round": round,
                                "payload_bytes": len(payload),
                                "rtt_micros": int((time.perf_counter() - started) * 1_000_000),
                            })
                        break
                    last_error = f"client {client_id} round {round}: {describe_response(response, expected)}"
            except Exception as exc:
                last_error = f"client {client_id} round {round}: {exc}"
            time.sleep(1)
        else:
            with samples_lock:
                errors.append(last_error or f"client {client_id} round {round}: timed out")

threads = [
    threading.Thread(target=client_worker, args=(client_id,), daemon=True)
    for client_id in range(1, client_count + 1)
]
echo_started = time.perf_counter()
for thread in threads:
    thread.start()
for thread in threads:
    thread.join()
echo_elapsed_micros = max(1, int((time.perf_counter() - echo_started) * 1_000_000))

expected_samples = client_count * rounds
if errors or len(samples) != expected_samples:
    detail = errors[0] if errors else f"expected {expected_samples} samples, got {len(samples)}"
    write_tunnel_echo_failure_evidence(detail)
    print(f"tunnel relay echo failed: {detail}", file=sys.stderr)
    raise SystemExit(1)

rtts = sorted(sample["rtt_micros"] for sample in samples)
total_payload_bytes = sum(sample["payload_bytes"] for sample in samples)
echo_throughput_mib_s = (
    (total_payload_bytes / 1024.0 / 1024.0) / (echo_elapsed_micros / 1_000_000.0)
)
client_p95s = []
for client_id in range(1, client_count + 1):
    client_rtts = sorted(
        sample["rtt_micros"] for sample in samples if sample["client"] == client_id
    )
    client_p95s.append(percentile(client_rtts, 95))
rtt_client_p95_spread_micros = max(client_p95s) - min(client_p95s) if len(client_p95s) > 1 else 0
summary = {
    "rtt_micros_p50": percentile(rtts, 50),
    "rtt_micros_p95": percentile(rtts, 95),
    "rtt_micros_p99": percentile(rtts, 99),
    "rtt_micros_max": rtts[-1],
    "rtt_client_p95_spread_micros": rtt_client_p95_spread_micros,
}

evidence_dir = os.path.dirname(evidence_file)
if evidence_dir:
    os.makedirs(evidence_dir, exist_ok=True)
with open(evidence_file, "w", encoding="utf-8") as fh:
    fh.write("# Tunnel Echo Evidence\n\n")
    fh.write(f"- profile: {profile}\n")
    fh.write(f"- rounds: {rounds}\n")
    fh.write(f"- clients: {client_count}\n")
    fh.write(f"- total_payload_bytes: {total_payload_bytes}\n")
    fh.write(f"- echo_elapsed_micros: {echo_elapsed_micros}\n")
    fh.write(f"- echo_throughput_mib_s: {echo_throughput_mib_s:.3f}\n")
    for key, value in summary.items():
        fh.write(f"- {key}: {value}\n")
    fh.write("\n## Client RTT P95\n\n")
    fh.write("| client | rtt_micros_p95 |\n")
    fh.write("| ---: | ---: |\n")
    for index, value in enumerate(client_p95s, start=1):
        fh.write(f"| {index} | {value} |\n")
    fh.write("\n## Samples\n\n")
    fh.write("| client | round | payload_bytes | rtt_micros |\n")
    fh.write("| ---: | ---: | ---: | ---: |\n")
    for sample in sorted(samples, key=lambda item: (item["client"], item["round"])):
        fh.write(f"| {sample['client']} | {sample['round']} | {sample['payload_bytes']} | {sample['rtt_micros']} |\n")

print(
    f"tunnel relay echo succeeded rounds={rounds} clients={client_count} profile={profile} "
    f"total_payload_bytes={total_payload_bytes} "
    f"echo_elapsed_micros={echo_elapsed_micros} "
    f"echo_throughput_mib_s={echo_throughput_mib_s:.3f} "
    f"rtt_micros_p50={summary['rtt_micros_p50']} "
    f"rtt_micros_p95={summary['rtt_micros_p95']} "
    f"rtt_micros_p99={summary['rtt_micros_p99']} "
    f"rtt_micros_max={summary['rtt_micros_max']} "
    f"rtt_client_p95_spread_micros={summary['rtt_client_p95_spread_micros']}"
)
raise SystemExit(0)
PY
    then
        die "tunnel relay echo verification failed$(log_tail_for_error)"
    fi
    printf '%s\n' "smoke: tunnel_relay_echo_succeeded rule_id=${TUNNEL_RULE_ID} listen_port=${TUNNEL_LISTEN_PORT} rounds=${KELICLOUD_TUNNEL_ECHO_ROUNDS} clients=${KELICLOUD_TUNNEL_ECHO_CLIENTS} profile=${KELICLOUD_TUNNEL_ECHO_PROFILE}" >>"${AGENT_LOG}"
    printf '%s\n' "smoke: tunnel_echo_evidence=${TUNNEL_ECHO_EVIDENCE_FILE}" >>"${AGENT_LOG}"
    log "Tunnel relay echo succeeded through 127.0.0.1:${TUNNEL_LISTEN_PORT} rounds=${KELICLOUD_TUNNEL_ECHO_ROUNDS} clients=${KELICLOUD_TUNNEL_ECHO_CLIENTS} profile=${KELICLOUD_TUNNEL_ECHO_PROFILE}"
    log "Tunnel echo evidence written to ${TUNNEL_ECHO_EVIDENCE_FILE}"
}

collect_ktp_live_canary_evidence() {
    local root="$1"
    if ! ktp_tcp_smoke_enabled; then
        return
    fi

    KTP_EVIDENCE_FILE="${SMOKE_LOG_DIR}/ktp-live-canary.evidence.md"
    wait_for_log "${AGENT_LOG}" "tunnel data diagnostics" "${KTP_DIAGNOSTICS_TIMEOUT_SECONDS}"
    KTP_LIVE_CANARY_AUTH_VERSION="${AGENT_TUNNEL_KTP_TCP_AUTH_VERSION}" \
    KTP_LIVE_CANARY_CARRIER="$(ktp_live_canary_carrier)" \
        bash "${root}/scripts/ktp-live-canary-evidence.sh" \
        --log-file "${AGENT_LOG}" \
        --evidence-file "${KTP_EVIDENCE_FILE}" \
        --min-lines "${KTP_LIVE_CANARY_MIN_LINES}"
    printf '%s\n' "smoke: ktp_live_canary_evidence=${KTP_EVIDENCE_FILE}" >>"${AGENT_LOG}"
    log "KTP live canary evidence written to ${KTP_EVIDENCE_FILE}"
}

print_summary() {
    local root="$1"
    local summary_file="${SMOKE_LOG_DIR}/agent.summary.md"
    # smoke-summary --require-pass compatibility gate
    (cd "${root}" && cargo run --locked --quiet --bin smoke-summary -- --require-pass "${AGENT_LOG}") | tee "${summary_file}"
}

record_agent_stayed_alive() {
    log "Recording agent stayed-alive smoke evidence"
    printf '%s\n' "live smoke duration reached" >>"${AGENT_LOG}"
}

main() {
    require_command git
    require_command go
    require_command cargo
    require_command curl
    require_command python3
    if [[ "${KELICLOUD_PREPARE_FRONTEND}" == "true" ]]; then
        require_command node
        require_command npm
    fi
    if ktp_tcp_smoke_enabled; then
        require_command bash
    fi
    require_positive_integer "KELICLOUD_TUNNEL_ECHO_ROUNDS" "${KELICLOUD_TUNNEL_ECHO_ROUNDS}"
    require_positive_integer "KELICLOUD_TUNNEL_ECHO_CLIENTS" "${KELICLOUD_TUNNEL_ECHO_CLIENTS}"
    require_non_negative_integer "KELICLOUD_TUNNEL_ECHO_PAYLOAD_BYTES" "${KELICLOUD_TUNNEL_ECHO_PAYLOAD_BYTES}"
    require_tunnel_echo_profile

    local root
    root="$(repo_root)"
    mkdir -p "${SMOKE_LOG_DIR}"
    if [[ -z "${SMOKE_WORK_DIR}" ]]; then
        SMOKE_WORK_DIR="$(mktemp -d)"
    else
        mkdir -p "${SMOKE_WORK_DIR}"
    fi
    WORK_DIR="${SMOKE_WORK_DIR}"

    configure_ktp_tcp_smoke
    set_stage "wait for MySQL"
    wait_for_mysql
    set_stage "prepare backend"
    prepare_backend
    set_stage "start backend"
    start_backend
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
    set_stage "restart agent after token recovery"
    restart_agent_after_token_recovery "${root}"
    set_stage "set tunnel smoke group"
    set_client_tunnel_group
    set_stage "enable CN connectivity probe"
    enable_cn_connectivity_probe
    set_stage "trigger exec"
    trigger_exec
    set_stage "trigger ping"
    trigger_ping
    set_stage "trigger terminal"
    trigger_terminal "${root}"
    set_stage "start tunnel echo server"
    start_tunnel_echo_server
    set_stage "create tunnel rule"
    create_tunnel_rule
    set_stage "verify tunnel relay echo"
    verify_tunnel_relay_echo
    if ktp_tcp_smoke_enabled; then
        set_stage "collect KTP canary evidence"
        collect_ktp_live_canary_evidence "${root}"
    fi
    record_agent_stayed_alive
    set_stage "print smoke summary"
    print_summary "${root}"

    log "Local backend smoke finished. Logs are in ${SMOKE_LOG_DIR}"
}

main "$@"
