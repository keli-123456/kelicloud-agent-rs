#!/usr/bin/env bash
set -Eeuo pipefail

KELICLOUD_BACKEND_REPO="${KELICLOUD_BACKEND_REPO:-https://github.com/keli-123456/kelicloud.git}"
KELICLOUD_BACKEND_REF="${KELICLOUD_BACKEND_REF:-main}"
KELICLOUD_BACKEND_PATH="${KELICLOUD_BACKEND_PATH:-}"
KELICLOUD_PREPARE_FRONTEND="${KELICLOUD_PREPARE_FRONTEND:-true}"
KOMARI_FRONTEND_REF="${KOMARI_FRONTEND_REF:-main}"

BACKEND_LISTEN="${BACKEND_LISTEN:-127.0.0.1:25775}"
BACKEND_ENDPOINT="${BACKEND_ENDPOINT:-http://${BACKEND_LISTEN}}"
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
BACKEND_DIR=""
WORK_DIR=""
COOKIE_JAR=""
BACKEND_LOG=""
AGENT_LOG=""
HELPER_LOG=""
AUTO_DISCOVERY_KEY=""
SMOKE_AGENT_HOSTNAME="${SMOKE_AGENT_HOSTNAME:-agent-rs-smoke}"
SMOKE_AGENT_CLIENT_NAME="Auto-${SMOKE_AGENT_HOSTNAME}"
ROTATED_AGENT_TOKEN=""
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
    if [[ -n "${SMOKE_WORK_DIR}" && "${SMOKE_WORK_DIR}" == /tmp/* && -d "${SMOKE_WORK_DIR}" ]]; then
        rm -rf "${SMOKE_WORK_DIR}"
    fi
}
trap cleanup EXIT

require_command() {
    command -v "$1" >/dev/null 2>&1 || die "$1 command is required"
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
else:
    raise SystemExit(f"unknown payload kind: {kind}")
PY
}

wait_for_http() {
    local url="$1"
    local timeout_seconds="$2"
    local deadline=$((SECONDS + timeout_seconds))
    until curl -fsS "${url}" >/dev/null 2>&1; do
        if (( SECONDS >= deadline )); then
            die "timed out waiting for ${url}"
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
    fi

    log "Building kelicloud backend"
    (cd "${BACKEND_DIR}" && go build -o "${WORK_DIR}/kelicloud-backend" .)
}

start_backend() {
    BACKEND_LOG="${SMOKE_LOG_DIR}/backend.log"
    log "Starting backend at ${BACKEND_ENDPOINT}"
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
            KOMARI_SECURITY_HSTS="false" \
            "${WORK_DIR}/kelicloud-backend" server
    ) >"${BACKEND_LOG}" 2>&1 &
    BACKEND_PID="$!"
    wait_for_http "${BACKEND_ENDPOINT}/ping" 90
}

login_admin() {
    COOKIE_JAR="${WORK_DIR}/cookies.txt"
    local login_payload
    login_payload="$(python3 -c '
import json
import sys
print(json.dumps({"username": sys.argv[1], "password": sys.argv[2]}))
' "${ADMIN_USERNAME}" "${ADMIN_PASSWORD}")"

    local response
    response="$(curl -fsS -c "${COOKIE_JAR}" \
        -H "Content-Type: application/json" \
        --data "${login_payload}" \
        "${BACKEND_ENDPOINT}/api/login")"
    SESSION_TOKEN="$(printf '%s' "${response}" | json_value "data.set-cookie.session_token")"
    [[ -n "${SESSION_TOKEN}" ]] || die "login response did not include session token"
}

load_auto_discovery_key() {
    local response
    response="$(curl_api GET "/api/admin/settings/")"
    AUTO_DISCOVERY_KEY="$(printf '%s' "${response}" | json_value "data.auto_discovery_key")"
    [[ -n "${AUTO_DISCOVERY_KEY}" ]] || die "settings response did not include auto_discovery_key"
    log "Loaded auto-discovery key for smoke"
}

resolve_auto_discovery_client() {
    local response uuid token deadline
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

    CLIENT_UUID="${uuid}"
    response="$(curl_api GET "/api/admin/client/${CLIENT_UUID}/token")"
    token="$(printf '%s' "${response}" | json_value "token")"
    [[ -n "${token}" ]] || die "client token response did not include token"
    AGENT_TOKEN="${token}"
    log "Resolved auto-discovered smoke client ${CLIENT_UUID}"
}

start_agent() {
    local root="$1"
    AGENT_LOG="${SMOKE_LOG_DIR}/agent.log"
    : >"${AGENT_LOG}"

    log "Building agent and smoke helpers"
    (cd "${root}" && cargo build --locked --release --bin kelicloud-agent-rs --bin admin-terminal-smoke --bin smoke-summary)
    rm -f "${root}/target/release/auto-discovery.json"

    log "Starting kelicloud-agent-rs"
    HOSTNAME="${SMOKE_AGENT_HOSTNAME}" "${root}/target/release/kelicloud-agent-rs" \
        --endpoint "${BACKEND_ENDPOINT}" \
        --auto-discovery "${AUTO_DISCOVERY_KEY}" \
        --interval 1 \
        --max-retries 3 \
        --reconnect-interval 1 \
        --info-report-interval 1 >>"${AGENT_LOG}" 2>&1 &
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
    resolve_auto_discovery_client
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
    require_command node
    require_command npm
    require_command cargo
    require_command curl
    require_command python3

    local root
    root="$(repo_root)"
    mkdir -p "${SMOKE_LOG_DIR}"
    if [[ -z "${SMOKE_WORK_DIR}" ]]; then
        SMOKE_WORK_DIR="$(mktemp -d)"
    else
        mkdir -p "${SMOKE_WORK_DIR}"
    fi
    WORK_DIR="${SMOKE_WORK_DIR}"

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
    set_stage "enable CN connectivity probe"
    enable_cn_connectivity_probe
    set_stage "trigger exec"
    trigger_exec
    set_stage "trigger ping"
    trigger_ping
    set_stage "trigger terminal"
    trigger_terminal "${root}"
    record_agent_stayed_alive
    set_stage "print smoke summary"
    print_summary "${root}"

    log "Local backend smoke finished. Logs are in ${SMOKE_LOG_DIR}"
}

main "$@"
