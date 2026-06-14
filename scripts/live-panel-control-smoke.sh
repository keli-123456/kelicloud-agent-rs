#!/usr/bin/env bash
set -Eeuo pipefail

ENDPOINT="${KELICLOUD_PANEL_ENDPOINT:-}"
CLIENT_UUID="${KELICLOUD_PANEL_CLIENT_UUID:-}"
COOKIE_HEADER="${KELICLOUD_PANEL_COOKIE:-}"
COOKIE_JAR="${KELICLOUD_PANEL_COOKIE_JAR:-}"
COMMAND_TEXT="${KELICLOUD_PANEL_EXEC_COMMAND:-printf 'kelicloud-agent-rs-live-exec-smoke\n'}"
PING_TARGET="${KELICLOUD_PANEL_PING_TARGET:-}"
TIMEOUT_SECONDS="${KELICLOUD_PANEL_CONTROL_TIMEOUT:-90}"
CHECK_JOURNAL="true"
JOURNAL_UNIT="kelicloud-agent-rs"
JOURNAL_SINCE=""
EXEC_TASK_ID=""
PING_TASK_ID=""

usage() {
    cat <<'EOF'
Live panel control-plane smoke for kelicloud-agent-rs.

Usage:
  KELICLOUD_PANEL_COOKIE='session_token=...' \
    scripts/live-panel-control-smoke.sh --endpoint URL --client UUID --ping-target HOST:PORT

Options:
  --endpoint URL             kelicloud panel endpoint, also read from KELICLOUD_PANEL_ENDPOINT
  --client UUID              target client UUID, also read from KELICLOUD_PANEL_CLIENT_UUID
  --cookie HEADER            raw Cookie header value, also read from KELICLOUD_PANEL_COOKIE
  --cookie-jar PATH          curl cookie jar path, also read from KELICLOUD_PANEL_COOKIE_JAR
  --command COMMAND          script command to execute on the client
  --ping-target HOST:PORT    TCP ping target
  --timeout SECONDS          wait timeout for API and journal evidence, default 90
  --journal-since VALUE      journalctl --since value, default: script start time
  --no-journal               only call panel APIs; do not check local journal
  --help                     Show this help

This helper uses the same live panel APIs as scripts/smoke-local-backend.sh:
POST /api/admin/task/exec and POST /api/admin/ping/add. Run it on the real
Linux host while kelicloud-agent-rs is active to also verify local journal
evidence: smoke: task_result_uploaded and smoke: ping_result_uploaded.
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

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --endpoint)
                need_value "$1" "${2:-}"
                ENDPOINT="$2"
                shift 2
                ;;
            --client)
                need_value "$1" "${2:-}"
                CLIENT_UUID="$2"
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
            --command)
                need_value "$1" "${2:-}"
                COMMAND_TEXT="$2"
                shift 2
                ;;
            --ping-target)
                need_value "$1" "${2:-}"
                PING_TARGET="$2"
                shift 2
                ;;
            --timeout)
                need_value "$1" "${2:-}"
                TIMEOUT_SECONDS="$2"
                shift 2
                ;;
            --journal-since)
                need_value "$1" "${2:-}"
                JOURNAL_SINCE="$2"
                shift 2
                ;;
            --no-journal)
                CHECK_JOURNAL="false"
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
    [[ -n "$ENDPOINT" ]] || die "--endpoint or KELICLOUD_PANEL_ENDPOINT is required"
    [[ -n "$CLIENT_UUID" ]] || die "--client or KELICLOUD_PANEL_CLIENT_UUID is required"
    [[ -n "$PING_TARGET" ]] || die "--ping-target or KELICLOUD_PANEL_PING_TARGET is required"
    [[ -n "$COOKIE_HEADER" || -n "$COOKIE_JAR" ]] || die "--cookie/KELICLOUD_PANEL_COOKIE or --cookie-jar/KELICLOUD_PANEL_COOKIE_JAR is required"
    [[ "$TIMEOUT_SECONDS" =~ ^[0-9]+$ ]] || die "--timeout must be whole seconds"
    [[ "$TIMEOUT_SECONDS" -gt 0 ]] || die "--timeout must be greater than zero"
    if [[ "$CHECK_JOURNAL" == "true" ]]; then
        command -v journalctl >/dev/null 2>&1 || die "journalctl is required unless --no-journal is used"
    fi
    command -v curl >/dev/null 2>&1 || die "curl is required"
    command -v python3 >/dev/null 2>&1 || die "python3 is required"
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
        "name": "agent-rs-real-host-smoke",
        "target": sys.argv[2],
        "type": "tcp",
        "interval": 1,
        "clients": [sys.argv[3]],
    }))
else:
    raise SystemExit(f"unknown payload kind: {kind}")
PY
}

curl_api() {
    local method="$1"
    local path="$2"
    local payload="${3:-}"
    local args=(-fsS -X "$method" -H "Content-Type: application/json")
    if [[ -n "$COOKIE_HEADER" ]]; then
        args+=(-H "Cookie: ${COOKIE_HEADER}")
    else
        args+=(-b "$COOKIE_JAR" -c "$COOKIE_JAR")
    fi
    if [[ -n "$payload" ]]; then
        args+=(--data "$payload")
    fi
    curl "${args[@]}" "${ENDPOINT%/}${path}"
}

wait_for_journal() {
    local needle="$1"
    local deadline=$((SECONDS + TIMEOUT_SECONDS))
    until journalctl -u kelicloud-agent-rs --since "$JOURNAL_SINCE" --no-pager 2>/dev/null | grep -Fq "$needle"; do
        if (( SECONDS >= deadline )); then
            log "journalctl -u ${JOURNAL_UNIT} --since ${JOURNAL_SINCE} --no-pager"
            journalctl -u "$JOURNAL_UNIT" --since "$JOURNAL_SINCE" --no-pager 2>/dev/null | tail -n 120 || true
            die "timed out waiting for journal evidence: ${needle}"
        fi
        sleep 1
    done
}

wait_for_exec_api_result() {
    local expected_mark="$1"
    local deadline=$((SECONDS + TIMEOUT_SECONDS))
    local result
    until result="$(curl_api GET "/api/admin/task/${EXEC_TASK_ID}/result/${CLIENT_UUID}" 2>/dev/null)" &&
        [[ "$result" == *"$expected_mark"* && "$result" == *'"exit_code":0'* ]]; do
        if (( SECONDS >= deadline )); then
            die "timed out waiting for exec task API result"
        fi
        sleep 1
    done
}

trigger_exec() {
    local payload response mark
    mark="kelicloud-agent-rs-live-exec-smoke"
    payload="$(json_payload exec "$COMMAND_TEXT" "$CLIENT_UUID")"
    response="$(curl_api POST "/api/admin/task/exec" "$payload")"
    EXEC_TASK_ID="$(printf '%s' "$response" | json_value "data.task_id")"
    [[ -n "$EXEC_TASK_ID" ]] || die "exec response did not include data.task_id"
    log "exec_task_id=${EXEC_TASK_ID}"
    wait_for_exec_api_result "$mark"
    if [[ "$CHECK_JOURNAL" == "true" ]]; then
        wait_for_journal "smoke: task_result_uploaded"
    fi
}

trigger_ping() {
    local payload response
    payload="$(json_payload ping "$PING_TARGET" "$CLIENT_UUID")"
    response="$(curl_api POST "/api/admin/ping/add" "$payload")"
    PING_TASK_ID="$(printf '%s' "$response" | json_value "data.task_id")"
    [[ -n "$PING_TASK_ID" ]] || die "ping response did not include data.task_id"
    log "ping_task_id=${PING_TASK_ID}"
    if [[ "$CHECK_JOURNAL" == "true" ]]; then
        wait_for_journal "smoke: ping_result_uploaded"
    fi
}

main() {
    parse_args "$@"
    validate_config
    if [[ -z "$JOURNAL_SINCE" ]]; then
        JOURNAL_SINCE="$(date -u '+%Y-%m-%d %H:%M:%S UTC')"
    fi

    log "Live panel control-plane smoke"
    log "Endpoint: ${ENDPOINT}"
    log "Client: ${CLIENT_UUID}"
    log "Ping target: ${PING_TARGET}"
    log "Journal check: ${CHECK_JOURNAL}"
    log "Journal since: ${JOURNAL_SINCE}"

    trigger_exec
    trigger_ping

    log "Live panel control-plane smoke passed."
}

main "$@"
