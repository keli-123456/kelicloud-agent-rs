#!/usr/bin/env bash
set -Eeuo pipefail

KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENTS="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENTS:-1 2 4 8}"
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

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
SMOKE_SCRIPT_REL="scripts/smoke-local-backend.sh"
SMOKE_SCRIPT="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_SMOKE_SCRIPT:-${REPO_ROOT}/${SMOKE_SCRIPT_REL}}"
MATRIX_LOG_ROOT=""
MATRIX_WORK_ROOT=""
MATRIX_SUMMARY_PATH=""

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

timestamp_millis() {
    python3 -c 'import time
print(int(time.time() * 1000))'
}

matrix_db_name() {
    local clients="$1"
    local prefix="${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_DB_PREFIX}"
    prefix="${prefix//[^a-zA-Z0-9_]/_}"
    printf '%s_clients_%s' "${prefix}" "${clients}"
}

matrix_identity_name() {
    local clients="$1"
    printf '%s-c%s' "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_IDENTITY_PREFIX}" "${clients}"
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
    printf '%s\n' "clients	rounds	profile	payload_bytes	status	elapsed_millis	log_dir	tunnel_evidence_file	ktp_evidence_file	total_payload_bytes	rtt_micros_p50	rtt_micros_p95	rtt_micros_p99	rtt_micros_max	rtt_client_p95_spread_micros	socket_read_batches	socket_read_frames	socket_read_max_batch_frames" >"${MATRIX_SUMMARY_PATH}"
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
    local clients="$1"
    local status="$2"
    local log_dir="$3"
    local elapsed_millis="$4"
    local tunnel_evidence_file="${log_dir}/tunnel-echo.evidence.md"
    local ktp_evidence_file="${log_dir}/ktp-live-canary.evidence.md"
    local tunnel_evidence_summary="-"
    local ktp_evidence_summary="-"

    if [[ -f "${tunnel_evidence_file}" ]]; then
        tunnel_evidence_summary="${tunnel_evidence_file}"
    fi
    if [[ -f "${ktp_evidence_file}" ]]; then
        ktp_evidence_summary="${ktp_evidence_file}"
    fi

    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "${clients}" \
        "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ROUNDS}" \
        "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_PROFILE}" \
        "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_PAYLOAD_BYTES}" \
        "${status}" \
        "${elapsed_millis}" \
        "${log_dir}" \
        "${tunnel_evidence_summary}" \
        "${ktp_evidence_summary}" \
        "$(plain_markdown_value "${tunnel_evidence_file}" "total_payload_bytes")" \
        "$(plain_markdown_value "${tunnel_evidence_file}" "rtt_micros_p50")" \
        "$(plain_markdown_value "${tunnel_evidence_file}" "rtt_micros_p95")" \
        "$(plain_markdown_value "${tunnel_evidence_file}" "rtt_micros_p99")" \
        "$(plain_markdown_value "${tunnel_evidence_file}" "rtt_micros_max")" \
        "$(plain_markdown_value "${tunnel_evidence_file}" "rtt_client_p95_spread_micros")" \
        "$(backtick_markdown_value "${ktp_evidence_file}" "socket_read_batches")" \
        "$(backtick_markdown_value "${ktp_evidence_file}" "socket_read_frames")" \
        "$(backtick_markdown_value "${ktp_evidence_file}" "socket_read_max_batch_frames")" >>"${MATRIX_SUMMARY_PATH}"
}

run_clients() {
    local clients="$1"
    local log_dir="${MATRIX_LOG_ROOT}/clients-${clients}"
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
    db_name="$(matrix_db_name "${clients}")"
    identity_name="$(matrix_identity_name "${clients}")"
    backend_listen="auto"
    backend_endpoint="auto"
    if [[ -n "${MATRIX_WORK_ROOT}" ]]; then
        work_dir="${MATRIX_WORK_ROOT}/clients-${clients}"
    fi

    echo "== ktp local backend tunnel clients=${clients} =="
    if [[ "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_DRY_RUN}" == "1" ]]; then
        printf 'dry_run: clients=%s KELICLOUD_SMOKE_KTP_TCP=true BACKEND_LISTEN=%s BACKEND_ENDPOINT=%s KOMARI_DB_NAME=%s SMOKE_AGENT_HOSTNAME=%s SMOKE_TUNNEL_GROUP=%s KELICLOUD_TUNNEL_ECHO_CLIENTS=%s KELICLOUD_TUNNEL_ECHO_ROUNDS=%s KELICLOUD_TUNNEL_ECHO_PROFILE=%s KELICLOUD_TUNNEL_ECHO_PAYLOAD_BYTES=%s KTP_LIVE_CANARY_MIN_MAX_BATCH_FRAMES=%s CLIENT_TIMEOUT_SECONDS=%s SMOKE_LOG_DIR=%s' \
            "${clients}" \
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
    write_summary_row "${clients}" "${status}" "${log_dir}" "${elapsed_millis}"
    return "${smoke_status}"
}

main() {
    require_positive_integer "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ROUNDS" "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ROUNDS}"
    require_positive_integer "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_PAYLOAD_BYTES" "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_PAYLOAD_BYTES}"
    require_positive_integer "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MIN_MAX_BATCH_FRAMES" "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_MIN_MAX_BATCH_FRAMES}"
    require_non_negative_integer "KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENT_TIMEOUT_SECONDS" "${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENT_TIMEOUT_SECONDS}"
    init_matrix_paths

    echo "== ktp local backend tunnel matrix =="
    echo "clients=${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENTS}"
    echo "rounds=${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_ROUNDS} profile=${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_PROFILE} payload_bytes=${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_PAYLOAD_BYTES}"
    echo "client_timeout_seconds=${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENT_TIMEOUT_SECONDS}"
    echo "log_dir=${MATRIX_LOG_ROOT}"
    if [[ -n "${MATRIX_WORK_ROOT}" ]]; then
        echo "work_dir=${MATRIX_WORK_ROOT}"
    fi
    echo "summary=${MATRIX_SUMMARY_PATH}"
    init_summary

    local clients
    for clients in ${KTP_LOCAL_BACKEND_TUNNEL_MATRIX_CLIENTS}; do
        run_clients "${clients}"
    done
}

main "$@"
