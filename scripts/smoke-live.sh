#!/usr/bin/env bash
set -euo pipefail

MODE="once"
DURATION_SECONDS="75"
BIN_PATH=""
BUILD_BINARY="true"
ENDPOINT="${AGENT_ENDPOINT:-}"
TOKEN="${AGENT_TOKEN:-}"
INTERVAL_SECONDS="5"
MAX_RETRIES="1"
RECONNECT_INTERVAL_SECONDS="3"
INFO_REPORT_INTERVAL_MINUTES="1"
INSECURE="false"
DISABLE_WEB_SSH="false"
CUSTOM_DNS=""
CF_ACCESS_CLIENT_ID="${AGENT_CF_ACCESS_CLIENT_ID:-}"
CF_ACCESS_CLIENT_SECRET="${AGENT_CF_ACCESS_CLIENT_SECRET:-}"
EXPECT_SUCCESS_LOG=""
EXTRA_ARGS=()

usage() {
    cat <<'EOF'
Live backend smoke test for kelicloud-agent-rs.

Usage:
  scripts/smoke-live.sh --endpoint URL --token TOKEN [options]
  AGENT_ENDPOINT=URL AGENT_TOKEN=TOKEN scripts/smoke-live.sh [options]

Modes:
  --mode once      Upload basic info, connect report websocket, send one report, then exit.
  --mode live      Run for --duration seconds so you can trigger ping/exec/terminal from the panel.

Options:
  --endpoint URL                 Backend endpoint. Also read from AGENT_ENDPOINT.
  --token TOKEN                  Agent token. Also read from AGENT_TOKEN.
  --duration SECONDS             Live-mode duration, default 75.
  --bin PATH                     Use an existing agent binary.
  --no-build                     Do not build target/release/kelicloud-agent-rs automatically.
  --interval SECONDS             Agent report interval, default 5.
  --max-retries COUNT            Agent max reconnect retries, default 1.
  --reconnect-interval SECONDS   Agent reconnect interval, default 3.
  --info-report-interval MINS    Agent basic-info refresh interval, default 1.
  --custom-dns SERVER            Pass --custom-dns to the agent.
  --cf-access-client-id ID       Pass Cloudflare Access client ID.
  --cf-access-client-secret SEC  Pass Cloudflare Access client secret.
  --insecure                     Pass --insecure to the agent.
  --disable-web-ssh              Pass --disable-web-ssh to the agent.
  --expect-success-log TEXT      Require TEXT to appear in captured logs.
  --help                         Show this help.

Examples:
  scripts/smoke-live.sh --endpoint https://panel.example.com --token TOKEN
  scripts/smoke-live.sh --mode live --duration 120 --endpoint https://panel.example.com --token TOKEN
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

redact_token() {
    local token="$1"
    local length="${#token}"
    if [[ "$length" -le 8 ]]; then
        printf '****'
        return
    fi

    printf '%s...%s' "${token:0:4}" "${token: -4}"
}

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --mode)
                need_value "$1" "${2:-}"
                MODE="$2"
                shift 2
                ;;
            --endpoint)
                need_value "$1" "${2:-}"
                ENDPOINT="$2"
                shift 2
                ;;
            --token)
                need_value "$1" "${2:-}"
                TOKEN="$2"
                shift 2
                ;;
            --duration)
                need_value "$1" "${2:-}"
                DURATION_SECONDS="$2"
                shift 2
                ;;
            --bin)
                need_value "$1" "${2:-}"
                BIN_PATH="$2"
                BUILD_BINARY="false"
                shift 2
                ;;
            --no-build)
                BUILD_BINARY="false"
                shift
                ;;
            --interval)
                need_value "$1" "${2:-}"
                INTERVAL_SECONDS="$2"
                shift 2
                ;;
            --max-retries)
                need_value "$1" "${2:-}"
                MAX_RETRIES="$2"
                shift 2
                ;;
            --reconnect-interval)
                need_value "$1" "${2:-}"
                RECONNECT_INTERVAL_SECONDS="$2"
                shift 2
                ;;
            --info-report-interval)
                need_value "$1" "${2:-}"
                INFO_REPORT_INTERVAL_MINUTES="$2"
                shift 2
                ;;
            --custom-dns)
                need_value "$1" "${2:-}"
                CUSTOM_DNS="$2"
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
            --insecure)
                INSECURE="true"
                shift
                ;;
            --disable-web-ssh)
                DISABLE_WEB_SSH="true"
                shift
                ;;
            --expect-success-log)
                need_value "$1" "${2:-}"
                EXPECT_SUCCESS_LOG="$2"
                shift 2
                ;;
            --help|-h)
                usage
                exit 0
                ;;
            --)
                shift
                EXTRA_ARGS+=("$@")
                break
                ;;
            *)
                die "unknown option: $1"
                ;;
        esac
    done
}

validate_config() {
    case "$MODE" in
        once|live) ;;
        *) die "--mode must be once or live" ;;
    esac

    [[ -n "$ENDPOINT" ]] || die "--endpoint or AGENT_ENDPOINT is required"
    [[ -n "$TOKEN" ]] || die "--token or AGENT_TOKEN is required"
    [[ "$DURATION_SECONDS" =~ ^[0-9]+$ ]] || die "--duration must be whole seconds"
    [[ "$DURATION_SECONDS" -gt 0 ]] || die "--duration must be greater than zero"
}

repo_root() {
    local script_dir
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    cd "${script_dir}/.." && pwd
}

resolve_binary() {
    local root="$1"
    if [[ -n "$BIN_PATH" ]]; then
        [[ -x "$BIN_PATH" ]] || die "binary is not executable: $BIN_PATH"
        return
    fi

    BIN_PATH="${root}/target/release/kelicloud-agent-rs"
    if [[ "$BUILD_BINARY" == "true" ]]; then
        log "Building release binary..."
        (cd "$root" && cargo build --locked --release)
    fi

    [[ -x "$BIN_PATH" ]] || die "binary is not executable: $BIN_PATH"
}

build_agent_command() {
    AGENT_COMMAND=(
        "$BIN_PATH"
        --endpoint "$ENDPOINT"
        --token "$TOKEN"
        --interval "$INTERVAL_SECONDS"
        --max-retries "$MAX_RETRIES"
        --reconnect-interval "$RECONNECT_INTERVAL_SECONDS"
        --info-report-interval "$INFO_REPORT_INTERVAL_MINUTES"
    )

    if [[ "$MODE" == "once" ]]; then
        AGENT_COMMAND+=(--once)
        if [[ -z "$EXPECT_SUCCESS_LOG" ]]; then
            EXPECT_SUCCESS_LOG="agent loop: completed"
        fi
    fi
    if [[ "$INSECURE" == "true" ]]; then
        AGENT_COMMAND+=(--insecure)
    fi
    if [[ "$DISABLE_WEB_SSH" == "true" ]]; then
        AGENT_COMMAND+=(--disable-web-ssh)
    fi
    if [[ -n "$CUSTOM_DNS" ]]; then
        AGENT_COMMAND+=(--custom-dns "$CUSTOM_DNS")
    fi
    if [[ -n "$CF_ACCESS_CLIENT_ID" ]]; then
        AGENT_COMMAND+=(--cf-access-client-id "$CF_ACCESS_CLIENT_ID")
    fi
    if [[ -n "$CF_ACCESS_CLIENT_SECRET" ]]; then
        AGENT_COMMAND+=(--cf-access-client-secret "$CF_ACCESS_CLIENT_SECRET")
    fi

    AGENT_COMMAND+=("${EXTRA_ARGS[@]}")
}

run_agent() {
    local log_file="$1"
    set +e
    if [[ "$MODE" == "live" ]]; then
        timeout --foreground "${DURATION_SECONDS}s" "${AGENT_COMMAND[@]}" 2>&1 | tee "$log_file"
    else
        timeout --foreground "${DURATION_SECONDS}s" "${AGENT_COMMAND[@]}" 2>&1 | tee "$log_file"
    fi
    local status="${PIPESTATUS[0]}"
    set -e
    return "$status"
}

check_result() {
    local status="$1"
    local log_file="$2"

    if [[ "$MODE" == "live" ]]; then
        if [[ "$status" -eq 124 ]]; then
            log "Live smoke duration reached; agent stayed running for ${DURATION_SECONDS}s."
        elif [[ "$status" -eq 0 ]]; then
            die "agent exited before live duration ended; inspect ${log_file}"
        else
            die "agent failed during live smoke with exit code ${status}; inspect ${log_file}"
        fi
    elif [[ "$status" -ne 0 ]]; then
        die "agent once smoke failed with exit code ${status}; inspect ${log_file}"
    fi

    if [[ -n "$EXPECT_SUCCESS_LOG" ]] && ! grep -Fq "$EXPECT_SUCCESS_LOG" "$log_file"; then
        die "expected log text not found: ${EXPECT_SUCCESS_LOG}; inspect ${log_file}"
    fi
}

main() {
    parse_args "$@"
    validate_config

    command -v timeout >/dev/null 2>&1 || die "timeout command is required"
    command -v tee >/dev/null 2>&1 || die "tee command is required"

    local root
    root="$(repo_root)"
    resolve_binary "$root"
    build_agent_command

    local log_file
    log_file="$(mktemp "${TMPDIR:-/tmp}/kelicloud-agent-rs-smoke.XXXXXX.log")"

    log "Smoke mode: ${MODE}"
    log "Endpoint: ${ENDPOINT}"
    log "Token: $(redact_token "$TOKEN")"
    log "Binary: ${BIN_PATH}"
    log "Log file: ${log_file}"
    if [[ "$MODE" == "live" ]]; then
        log "While this runs, trigger ping, script exec, or terminal from the kelicloud panel."
    fi

    local status
    if run_agent "$log_file"; then
        status=0
    else
        status="$?"
    fi

    check_result "$status" "$log_file"
    log "Smoke test finished. Log file: ${log_file}"
}

main "$@"
