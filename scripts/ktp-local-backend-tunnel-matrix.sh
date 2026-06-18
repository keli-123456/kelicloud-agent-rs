#!/usr/bin/env bash
set -Eeuo pipefail

KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENTS="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENTS:-1 2 4 8}"
KTP_LOCAL_BACKEND_TUNNEL_MATRIX_RELAY_BATCH_POLICIES="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_RELAY_BATCH_POLICIES:-fixed}"
KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ROUNDS="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ROUNDS:-8}"
KTP_LOCAL_BACKEND_TUNNEL_MATRIX_PROFILE="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_PROFILE:-rdp-like}"
KTP_LOCAL_BACKEND_TUNNEL_MATRIX_PAYLOAD_BYTES="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_PAYLOAD_BYTES:-8192}"
KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MIN_MAX_BATCH_FRAMES="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MIN_MAX_BATCH_FRAMES:-2}"
KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENT_TIMEOUT_SECONDS="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENT_TIMEOUT_SECONDS:-900}"
KTP_LOCAL_BACKEND_TUNNEL_MATRIX_LOG_DIR="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_LOG_DIR:-smoke-logs/local-backend-tunnel-matrix}"
KTP_LOCAL_BACKEND_TUNNEL_MATRIX_WORK_DIR="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_WORK_DIR:-}"
KTP_LOCAL_BACKEND_TUNNEL_MATRIX_DRY_RUN="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_DRY_RUN:-0}"
KTP_LOCAL_BACKEND_TUNNEL_MATRIX_SUMMARY="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_SUMMARY:-}"
KTP_LOCAL_BACKEND_TUNNEL_MATRIX_SMOKE_SCRIPT="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_SMOKE_SCRIPT:-}"
KTP_LOCAL_BACKEND_TUNNEL_MATRIX_DB_PREFIX="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_DB_PREFIX:-${KOMARI_DB_NAME:-komari_tunnel_matrix}}"
KTP_LOCAL_BACKEND_TUNNEL_MATRIX_IDENTITY_PREFIX="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_IDENTITY_PREFIX:-agent-rs-tunnel-matrix}"
KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MAX_RTT_P95_MICROS="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MAX_RTT_P95_MICROS:-}"
KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MAX_CLIENT_P95_SPREAD_MICROS="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MAX_CLIENT_P95_SPREAD_MICROS:-}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
SMOKE_SCRIPT_REL="scripts/smoke-local-backend.sh"
SMOKE_SCRIPT="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_SMOKE_SCRIPT:-${REPO_ROOT}/${SMOKE_SCRIPT_REL}}"
MATRIX_LOG_ROOT=""
MATRIX_WORK_ROOT=""
MATRIX_SUMMARY_PATH=""
MATRIX_GATE_FAILURES=0

trim_trailing_slash() {
    local value="$1"
    while [[ "${value}" == */ && "${value}" != "/" ]]; do
        value="${value%/}"
    done
    printf '%s' "${value}"
}

require_positive_integer() {
    local name="$1"
    local value="$2"
    [[ "${value}" =~ ^[1-9][0-9]*$ ]] || {
        echo "${name} must be a positive integer" >&2
        return 2
    }
}

require_non_negative_integer() {
    local name="$1"
    local value="$2"
    [[ "${value}" =~ ^[0-9]+$ ]] || {
        echo "${name} must be a non-negative integer" >&2
        return 2
    }
}

record_gate_failure() {
    echo "KTP tunnel matrix performance gate failed: $*" >&2
    MATRIX_GATE_FAILURES=$((MATRIX_GATE_FAILURES + 1))
}

check_max_metric() {
    local policy="$1"
    local clients="$2"
    local metric="$3"
    local value="$4"
    local max="$5"

    if [[ -z "${max}" ]]; then
        return
    fi
    if [[ -z "${value}" || "${value}" == "-" ]]; then
        record_gate_failure "missing ${metric} for policy=${policy} clients=${clients}"
        return
    fi
    if ! [[ "${value}" =~ ^[0-9]+$ ]]; then
        record_gate_failure "non-numeric ${metric}=${value} for policy=${policy} clients=${clients}"
        return
    fi
    if ((value > max)); then
        record_gate_failure "${metric} ${value} exceeds max ${max} for policy=${policy} clients=${clients}"
    fi
}

validate_relay_batch_policy() {
    local policy="$1"
    if [[ "${policy}" != "fixed" && "${policy}" != "adaptive" ]]; then
        echo "invalid relay batch policy: ${policy}" >&2
        return 2
    fi
}

policy_path_component() {
    local policy="$1"
    printf '%s' "${policy//[^a-zA-Z0-9_]/_}"
}

timestamp_millis() {
    python3 -c 'import time
print(int(time.time() * 1000))'
}

matrix_db_name() {
    local policy="$1"
    local clients="$2"
    local prefix="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_DB_PREFIX}"
    local policy_component
    policy_component="$(policy_path_component "${policy}")"
    prefix="${prefix//[^a-zA-Z0-9_]/_}"
    printf '%s_%s_clients_%s' "${prefix}" "${policy_component}" "${clients}"
}

matrix_identity_name() {
    local policy="$1"
    local clients="$2"
    local policy_component
    policy_component="$(policy_path_component "${policy}")"
    printf '%s-%s-c%s' "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_IDENTITY_PREFIX}" "${policy_component}" "${clients}"
}

pick_free_tcp_port() {
    python3 -c 'import socket
sock = socket.socket()
sock.bind(("127.0.0.1", 0))
print(sock.getsockname()[1])
sock.close()'
}

init_matrix_paths() {
    MATRIX_LOG_ROOT="$(trim_trailing_slash "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_LOG_DIR}")"
    MATRIX_WORK_ROOT="$(trim_trailing_slash "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_WORK_DIR}")"
    if [[ -n "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_SUMMARY}" ]]; then
        MATRIX_SUMMARY_PATH="$(trim_trailing_slash "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_SUMMARY}")"
    else
        MATRIX_SUMMARY_PATH="${MATRIX_LOG_ROOT}/matrix-summary.tsv"
    fi
}

init_summary() {
    if [[ "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_DRY_RUN}" == "1" ]]; then
        return
    fi
    mkdir -p "$(dirname "${MATRIX_SUMMARY_PATH}")"
    printf '%s\n' "relay_batch_policy	clients	rounds	profile	payload_bytes	status	elapsed_millis	log_dir	tunnel_evidence_file	ktp_evidence_file	total_payload_bytes	rtt_micros_p50	rtt_micros_p95	rtt_micros_p99	rtt_micros_max	rtt_client_p95_spread_micros	socket_read_batches	socket_read_frames	socket_read_max_batch_frames" >"${MATRIX_SUMMARY_PATH}"
}

plain_markdown_value() {
    local file="$1"
    local key="$2"
    if [[ ! -f "${file}" ]]; then
        printf '%s' "-"
        return
    fi
    local value
    value="$(grep -E "^- ${key}:" "${file}" | head -n 1 | sed -E "s/^- ${key}:[[:space:]]*//" || true)"
    if [[ -z "${value}" ]]; then
        printf '%s' "-"
    else
        printf '%s' "${value}"
    fi
}

backtick_markdown_value() {
    local file="$1"
    local key="$2"
    if [[ ! -f "${file}" ]]; then
        printf '%s' "-"
        return
    fi
    local value
    value="$(grep -E "^- \`${key}\`:" "${file}" | head -n 1 | sed -E "s/^- \`${key}\`:[[:space:]]*\`?([^\`]*)\`?.*/\1/" || true)"
    if [[ -z "${value}" ]]; then
        printf '%s' "-"
    else
        printf '%s' "${value}"
    fi
}

write_summary_row() {
    local policy="$1"
    local clients="$2"
    local status="$3"
    local log_dir="$4"
    local elapsed_millis="$5"
    local tunnel_evidence_file="${log_dir}/tunnel-echo.evidence.md"
    local ktp_evidence_file="${log_dir}/ktp-live-canary.evidence.md"
    local tunnel_evidence_summary="-"
    local ktp_evidence_summary="-"
    local total_payload_bytes rtt_micros_p50 rtt_micros_p95 rtt_micros_p99 rtt_micros_max
    local rtt_client_p95_spread_micros socket_read_batches socket_read_frames socket_read_max_batch_frames

    if [[ -f "${tunnel_evidence_file}" ]]; then
        tunnel_evidence_summary="${tunnel_evidence_file}"
    fi
    if [[ -f "${ktp_evidence_file}" ]]; then
        ktp_evidence_summary="${ktp_evidence_file}"
    fi

    total_payload_bytes="$(plain_markdown_value "${tunnel_evidence_file}" "total_payload_bytes")"
    rtt_micros_p50="$(plain_markdown_value "${tunnel_evidence_file}" "rtt_micros_p50")"
    rtt_micros_p95="$(plain_markdown_value "${tunnel_evidence_file}" "rtt_micros_p95")"
    rtt_micros_p99="$(plain_markdown_value "${tunnel_evidence_file}" "rtt_micros_p99")"
    rtt_micros_max="$(plain_markdown_value "${tunnel_evidence_file}" "rtt_micros_max")"
    rtt_client_p95_spread_micros="$(plain_markdown_value "${tunnel_evidence_file}" "rtt_client_p95_spread_micros")"
    socket_read_batches="$(backtick_markdown_value "${ktp_evidence_file}" "socket_read_batches")"
    socket_read_frames="$(backtick_markdown_value "${ktp_evidence_file}" "socket_read_frames")"
    socket_read_max_batch_frames="$(backtick_markdown_value "${ktp_evidence_file}" "socket_read_max_batch_frames")"

    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "${policy}" \
        "${clients}" \
        "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ROUNDS}" \
        "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_PROFILE}" \
        "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_PAYLOAD_BYTES}" \
        "${status}" \
        "${elapsed_millis}" \
        "${log_dir}" \
        "${tunnel_evidence_summary}" \
        "${ktp_evidence_summary}" \
        "${total_payload_bytes}" \
        "${rtt_micros_p50}" \
        "${rtt_micros_p95}" \
        "${rtt_micros_p99}" \
        "${rtt_micros_max}" \
        "${rtt_client_p95_spread_micros}" \
        "${socket_read_batches}" \
        "${socket_read_frames}" \
        "${socket_read_max_batch_frames}" >>"${MATRIX_SUMMARY_PATH}"

    if [[ "${status}" == "pass" ]]; then
        check_max_metric "${policy}" "${clients}" "rtt_micros_p95" "${rtt_micros_p95}" "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MAX_RTT_P95_MICROS}"
        check_max_metric "${policy}" "${clients}" "rtt_client_p95_spread_micros" "${rtt_client_p95_spread_micros}" "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MAX_CLIENT_P95_SPREAD_MICROS}"
    fi
}

run_clients() {
    local policy="$1"
    local clients="$2"
    local policy_component
    policy_component="$(policy_path_component "${policy}")"
    local log_dir="${MATRIX_LOG_ROOT}/${policy_component}/clients-${clients}"
    local work_dir=""
    local db_name
    local identity_name
    local backend_listen
    local backend_endpoint
    local smoke_status=0
    local status
    local started_millis
    local ended_millis
    local elapsed_millis

    require_positive_integer "client count" "${clients}"
    log_dir="$(trim_trailing_slash "${log_dir}")"
    db_name="$(matrix_db_name "${policy}" "${clients}")"
    identity_name="$(matrix_identity_name "${policy}" "${clients}")"
    backend_listen="auto"
    backend_endpoint="auto"
    if [[ -n "${MATRIX_WORK_ROOT}" ]]; then
        work_dir="${MATRIX_WORK_ROOT}/${policy_component}/clients-${clients}"
    fi

    echo "== ktp local backend tunnel relay_batch_policy=${policy} clients=${clients} =="
    if [[ "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_DRY_RUN}" == "1" ]]; then
        printf 'dry_run: relay_batch_policy=%s clients=%s KELICLOUD_SMOKE_KTP_TCP=true AGENT_TUNNEL_KTP_RELAY_BATCH_POLICY=%s BACKEND_LISTEN=%s BACKEND_ENDPOINT=%s KOMARI_DB_NAME=%s SMOKE_AGENT_HOSTNAME=%s SMOKE_TUNNEL_GROUP=%s KELICLOUD_TUNNEL_ECHO_CLIENTS=%s KELICLOUD_TUNNEL_ECHO_ROUNDS=%s KELICLOUD_TUNNEL_ECHO_PROFILE=%s KELICLOUD_TUNNEL_ECHO_PAYLOAD_BYTES=%s KTP_LIVE_CANARY_MIN_MAX_BATCH_FRAMES=%s CLIENT_TIMEOUT_SECONDS=%s SMOKE_LOG_DIR=%s' \
            "${policy}" \
            "${clients}" \
            "${policy}" \
            "${backend_listen}" \
            "${backend_endpoint}" \
            "${db_name}" \
            "${identity_name}" \
            "${identity_name}" \
            "${clients}" \
            "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ROUNDS}" \
            "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_PROFILE}" \
            "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_PAYLOAD_BYTES}" \
            "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MIN_MAX_BATCH_FRAMES}" \
            "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENT_TIMEOUT_SECONDS}" \
            "${log_dir}"
        if [[ -n "${work_dir}" ]]; then
            printf ' SMOKE_WORK_DIR=%s' "${work_dir}"
        fi
        printf ' bash %s\n' "${SMOKE_SCRIPT_REL}"
        return 0
    fi

    mkdir -p "${log_dir}"
    if [[ -n "${work_dir}" ]]; then
        mkdir -p "${work_dir}"
    fi
    backend_listen="127.0.0.1:$(pick_free_tcp_port)"
    backend_endpoint="http://${backend_listen}"

    started_millis="$(timestamp_millis)"
    (
        export KELICLOUD_SMOKE_KTP_TCP=true
        export AGENT_TUNNEL_KTP_RELAY_BATCH_POLICY="${policy}"
        export BACKEND_LISTEN="${backend_listen}"
        export BACKEND_ENDPOINT="${backend_endpoint}"
        export KOMARI_DB_NAME="${db_name}"
        export SMOKE_AGENT_HOSTNAME="${identity_name}"
        export SMOKE_TUNNEL_GROUP="${identity_name}"
        export KELICLOUD_TUNNEL_ECHO_CLIENTS="${clients}"
        export KELICLOUD_TUNNEL_ECHO_ROUNDS="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ROUNDS}"
        export KELICLOUD_TUNNEL_ECHO_PROFILE="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_PROFILE}"
        export KELICLOUD_TUNNEL_ECHO_PAYLOAD_BYTES="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_PAYLOAD_BYTES}"
        export KTP_LIVE_CANARY_MIN_MAX_BATCH_FRAMES="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MIN_MAX_BATCH_FRAMES}"
        export SMOKE_LOG_DIR="${log_dir}"
        if [[ -n "${work_dir}" ]]; then
            export SMOKE_WORK_DIR="${work_dir}"
        fi
        if [[ "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENT_TIMEOUT_SECONDS}" == "0" ]]; then
            bash "${SMOKE_SCRIPT}"
        else
            timeout "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENT_TIMEOUT_SECONDS}s" bash "${SMOKE_SCRIPT}"
        fi
    ) || smoke_status=$?
    ended_millis="$(timestamp_millis)"
    elapsed_millis="$((ended_millis - started_millis))"

    if [[ "${smoke_status}" == "0" ]]; then
        status="pass"
    elif [[ "${smoke_status}" == "124" ]]; then
        status="timeout"
    else
        status="fail"
    fi
    echo "clients=${clients} status=${status} elapsed_millis=${elapsed_millis}"
    write_summary_row "${policy}" "${clients}" "${status}" "${log_dir}" "${elapsed_millis}"
    return "${smoke_status}"
}

main() {
    require_positive_integer "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ROUNDS" "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ROUNDS}"
    require_positive_integer "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_PAYLOAD_BYTES" "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_PAYLOAD_BYTES}"
    require_positive_integer "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MIN_MAX_BATCH_FRAMES" "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MIN_MAX_BATCH_FRAMES}"
    require_non_negative_integer "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENT_TIMEOUT_SECONDS" "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENT_TIMEOUT_SECONDS}"
    if [[ -n "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MAX_RTT_P95_MICROS}" ]]; then
        require_positive_integer "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MAX_RTT_P95_MICROS" "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MAX_RTT_P95_MICROS}"
    fi
    if [[ -n "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MAX_CLIENT_P95_SPREAD_MICROS}" ]]; then
        require_non_negative_integer "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MAX_CLIENT_P95_SPREAD_MICROS" "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MAX_CLIENT_P95_SPREAD_MICROS}"
    fi
    init_matrix_paths

    echo "== ktp local backend tunnel matrix =="
    echo "relay_batch_policies=${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_RELAY_BATCH_POLICIES}"
    echo "clients=${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENTS}"
    echo "rounds=${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ROUNDS} profile=${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_PROFILE} payload_bytes=${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_PAYLOAD_BYTES}"
    echo "client_timeout_seconds=${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENT_TIMEOUT_SECONDS}"
    echo "log_dir=${MATRIX_LOG_ROOT}"
    if [[ -n "${MATRIX_WORK_ROOT}" ]]; then
        echo "work_dir=${MATRIX_WORK_ROOT}"
    fi
    echo "summary=${MATRIX_SUMMARY_PATH}"
    init_summary

    local policy clients
    for policy in ${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_RELAY_BATCH_POLICIES}; do
        validate_relay_batch_policy "${policy}"
        for clients in ${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENTS}; do
            run_clients "${policy}" "${clients}"
        done
    done
    if ((MATRIX_GATE_FAILURES > 0)); then
        echo "performance_gate_failures=${MATRIX_GATE_FAILURES}" >&2
        exit 3
    fi
}

main "$@"
