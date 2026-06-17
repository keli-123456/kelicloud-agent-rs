#!/usr/bin/env bash
set -Eeuo pipefail

KELICLOUD_LOCAL_BACKEND_MATRIX_CARRIERS="${KELICLOUD_LOCAL_BACKEND_MATRIX_CARRIERS:-websocket ktp_tcp}"
KELICLOUD_LOCAL_BACKEND_MATRIX_LOG_DIR="${KELICLOUD_LOCAL_BACKEND_MATRIX_LOG_DIR:-smoke-logs/local-backend-matrix}"
KELICLOUD_LOCAL_BACKEND_MATRIX_WORK_DIR="${KELICLOUD_LOCAL_BACKEND_MATRIX_WORK_DIR:-}"
KELICLOUD_LOCAL_BACKEND_MATRIX_DRY_RUN="${KELICLOUD_LOCAL_BACKEND_MATRIX_DRY_RUN:-0}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
SMOKE_SCRIPT_REL="scripts/smoke-local-backend.sh"
SMOKE_SCRIPT="${REPO_ROOT}/${SMOKE_SCRIPT_REL}"

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

run_carrier() {
    local carrier="$1"
    local ktp_enabled log_root work_root log_dir work_dir

    ktp_enabled="$(carrier_ktp_enabled "${carrier}")"
    log_root="$(trim_trailing_slash "${KELICLOUD_LOCAL_BACKEND_MATRIX_LOG_DIR}")"
    work_root="$(trim_trailing_slash "${KELICLOUD_LOCAL_BACKEND_MATRIX_WORK_DIR}")"
    log_dir="${log_root}/${carrier}"
    work_dir=""
    if [[ -n "${work_root}" ]]; then
        work_dir="${work_root}/${carrier}"
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
    )
}

main() {
    echo "== ktp local backend carrier matrix =="
    echo "carriers=${KELICLOUD_LOCAL_BACKEND_MATRIX_CARRIERS}"
    echo "log_dir=${KELICLOUD_LOCAL_BACKEND_MATRIX_LOG_DIR}"
    if [[ -n "${KELICLOUD_LOCAL_BACKEND_MATRIX_WORK_DIR}" ]]; then
        echo "work_dir=${KELICLOUD_LOCAL_BACKEND_MATRIX_WORK_DIR}"
    fi

    local carrier
    for carrier in ${KELICLOUD_LOCAL_BACKEND_MATRIX_CARRIERS}; do
        run_carrier "${carrier}"
    done
}

main "$@"
