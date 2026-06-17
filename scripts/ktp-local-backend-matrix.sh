#!/usr/bin/env bash
set -Eeuo pipefail

KELICLOUD_LOCAL_BACKEND_MATRIX_CARRIERS="${KELICLOUD_LOCAL_BACKEND_MATRIX_CARRIERS:-websocket ktp_tcp}"
KELICLOUD_LOCAL_BACKEND_MATRIX_LOG_DIR="${KELICLOUD_LOCAL_BACKEND_MATRIX_LOG_DIR:-smoke-logs/local-backend-matrix}"
KELICLOUD_LOCAL_BACKEND_MATRIX_WORK_DIR="${KELICLOUD_LOCAL_BACKEND_MATRIX_WORK_DIR:-}"
KELICLOUD_LOCAL_BACKEND_MATRIX_DRY_RUN="${KELICLOUD_LOCAL_BACKEND_MATRIX_DRY_RUN:-0}"
KELICLOUD_LOCAL_BACKEND_MATRIX_SUMMARY="${KELICLOUD_LOCAL_BACKEND_MATRIX_SUMMARY:-}"
KELICLOUD_LOCAL_BACKEND_MATRIX_SMOKE_SCRIPT="${KELICLOUD_LOCAL_BACKEND_MATRIX_SMOKE_SCRIPT:-}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
SMOKE_SCRIPT_REL="scripts/smoke-local-backend.sh"
SMOKE_SCRIPT="${KELICLOUD_LOCAL_BACKEND_MATRIX_SMOKE_SCRIPT:-${REPO_ROOT}/${SMOKE_SCRIPT_REL}}"
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

carrier_ktp_enabled() {
    local carrier="$1"
    case "${carrier}" in
        websocket)
            KELICLOUD_SMOKE_KTP_TCP=false
            ;;
        ktp_tcp)
            KELICLOUD_SMOKE_KTP_TCP=true
            ;;
        *)
            echo "unknown carrier: ${carrier}" >&2
            return 2
            ;;
    esac
    printf '%s' "${KELICLOUD_SMOKE_KTP_TCP}"
}

init_matrix_paths() {
    MATRIX_LOG_ROOT="$(trim_trailing_slash "${KELICLOUD_LOCAL_BACKEND_MATRIX_LOG_DIR}")"
    MATRIX_WORK_ROOT="$(trim_trailing_slash "${KELICLOUD_LOCAL_BACKEND_MATRIX_WORK_DIR}")"
    if [[ -n "${KELICLOUD_LOCAL_BACKEND_MATRIX_SUMMARY}" ]]; then
        MATRIX_SUMMARY_PATH="$(trim_trailing_slash "${KELICLOUD_LOCAL_BACKEND_MATRIX_SUMMARY}")"
    else
        MATRIX_SUMMARY_PATH="${MATRIX_LOG_ROOT}/matrix-summary.tsv"
    fi
}

init_summary() {
    if [[ "${KELICLOUD_LOCAL_BACKEND_MATRIX_DRY_RUN}" == "1" ]]; then
        return
    fi
    mkdir -p "$(dirname "${MATRIX_SUMMARY_PATH}")"
    printf '%s\n' "carrier	ktp_tcp	status	log_dir	summary_file	ktp_evidence_file" >"${MATRIX_SUMMARY_PATH}"
}

write_summary_row() {
    local carrier="$1"
    local ktp_enabled="$2"
    local status="$3"
    local log_dir="$4"
    local summary_file="${log_dir}/agent.summary.md"
    local ktp_evidence_file="-"

    if [[ ! -f "${summary_file}" ]]; then
        summary_file="-"
    fi
    if [[ "${ktp_enabled}" == "true" && -f "${log_dir}/ktp-live-canary.evidence.md" ]]; then
        ktp_evidence_file="${log_dir}/ktp-live-canary.evidence.md"
    fi

    printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
        "${carrier}" \
        "${ktp_enabled}" \
        "${status}" \
        "${log_dir}" \
        "${summary_file}" \
        "${ktp_evidence_file}" >>"${MATRIX_SUMMARY_PATH}"
}

run_carrier() {
    local carrier="$1"
    local ktp_enabled log_dir work_dir smoke_status status

    ktp_enabled="$(carrier_ktp_enabled "${carrier}")"
    log_dir="${MATRIX_LOG_ROOT}/${carrier}"
    work_dir=""
    if [[ -n "${MATRIX_WORK_ROOT}" ]]; then
        work_dir="${MATRIX_WORK_ROOT}/${carrier}"
    fi

    echo "== local backend smoke carrier=${carrier} =="
    if [[ "${KELICLOUD_LOCAL_BACKEND_MATRIX_DRY_RUN}" == "1" ]]; then
        printf 'dry_run: carrier=%s KELICLOUD_SMOKE_KTP_TCP=%s SMOKE_LOG_DIR=%s' \
            "${carrier}" "${ktp_enabled}" "${log_dir}"
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

    (
        export KELICLOUD_SMOKE_KTP_TCP="${ktp_enabled}"
        export SMOKE_LOG_DIR="${log_dir}"
        if [[ -n "${work_dir}" ]]; then
            export SMOKE_WORK_DIR="${work_dir}"
        fi
        bash "${SMOKE_SCRIPT}"
    ) || smoke_status=$?
    smoke_status="${smoke_status:-0}"
    if [[ "${smoke_status}" == "0" ]]; then
        status="pass"
    else
        status="fail"
    fi
    write_summary_row "${carrier}" "${ktp_enabled}" "${status}" "${log_dir}"
    return "${smoke_status}"
}

main() {
    init_matrix_paths
    echo "== ktp local backend carrier matrix =="
    echo "carriers=${KELICLOUD_LOCAL_BACKEND_MATRIX_CARRIERS}"
    echo "log_dir=${MATRIX_LOG_ROOT}"
    if [[ -n "${MATRIX_WORK_ROOT}" ]]; then
        echo "work_dir=${MATRIX_WORK_ROOT}"
    fi
    echo "summary=${MATRIX_SUMMARY_PATH}"
    init_summary

    local carrier
    for carrier in ${KELICLOUD_LOCAL_BACKEND_MATRIX_CARRIERS}; do
        run_carrier "${carrier}"
    done
}

main "$@"
