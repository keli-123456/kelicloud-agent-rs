#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "ktp carrier matrix is Linux-only" >&2
  exit 2
fi

DIRECTIONS="${KTP_CARRIER_MATRIX_DIRECTIONS:-client-to-relay relay-to-client-batch-read}"
RUNS="${KTP_CARRIER_MATRIX_RUNS:-3}"
FRAMES_LIST="${KTP_CARRIER_MATRIX_FRAMES:-512 4096}"
PAYLOAD_BYTES_LIST="${KTP_CARRIER_MATRIX_PAYLOAD_BYTES:-1024 4096 16384}"
DRY_RUN="${KTP_CARRIER_MATRIX_DRY_RUN:-0}"
CSV_PATH="${KTP_CARRIER_MATRIX_CSV:-}"

csv_header() {
  printf '%s\n' "direction,runs,frames,payload_bytes,read_batch_frames,elapsed_ms_min,elapsed_ms_median,elapsed_ms_max,throughput_mib_s_min,throughput_mib_s_median,throughput_mib_s_max"
}

metric_value() {
  local output="$1"
  local key="$2"
  printf '%s\n' "${output}" | tr ' ' '\n' | awk -F= -v wanted="${key}" '$1 == wanted { print $2; exit }'
}

first_metric_value() {
  local output="$1"
  shift
  local key value
  for key in "$@"; do
    value="$(metric_value "${output}" "${key}")"
    if [[ -n "${value}" ]]; then
      printf '%s\n' "${value}"
      return 0
    fi
  done
  return 1
}

required_metric_value() {
  local output="$1"
  shift
  local value
  if ! value="$(first_metric_value "${output}" "$@")"; then
    echo "missing metric in ktp-tunnel-bench output: $*" >&2
    return 1
  fi
  printf '%s\n' "${value}"
}

write_csv_row() {
  local output="$1"
  local direction frames payload_bytes
  local read_batch_frames
  local elapsed_ms_min elapsed_ms_median elapsed_ms_max
  local throughput_mib_s_min throughput_mib_s_median throughput_mib_s_max

  direction="$(required_metric_value "${output}" direction)"
  frames="$(required_metric_value "${output}" frames)"
  payload_bytes="$(required_metric_value "${output}" payload_bytes)"
  read_batch_frames="$(metric_value "${output}" read_batch_frames)"
  if [[ -z "${read_batch_frames}" ]]; then
    read_batch_frames="0"
  fi
  elapsed_ms_min="$(required_metric_value "${output}" elapsed_ms_min elapsed_ms)"
  elapsed_ms_median="$(required_metric_value "${output}" elapsed_ms_median elapsed_ms)"
  elapsed_ms_max="$(required_metric_value "${output}" elapsed_ms_max elapsed_ms)"
  throughput_mib_s_min="$(required_metric_value "${output}" throughput_mib_s_min throughput_mib_s)"
  throughput_mib_s_median="$(required_metric_value "${output}" throughput_mib_s_median throughput_mib_s)"
  throughput_mib_s_max="$(required_metric_value "${output}" throughput_mib_s_max throughput_mib_s)"

  printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
    "${direction}" \
    "${RUNS}" \
    "${frames}" \
    "${payload_bytes}" \
    "${read_batch_frames}" \
    "${elapsed_ms_min}" \
    "${elapsed_ms_median}" \
    "${elapsed_ms_max}" \
    "${throughput_mib_s_min}" \
    "${throughput_mib_s_median}" \
    "${throughput_mib_s_max}" >>"${CSV_PATH}"
}

echo "== ktp carrier matrix =="
echo "directions=${DIRECTIONS} runs=${RUNS} frames=${FRAMES_LIST} payload_bytes=${PAYLOAD_BYTES_LIST}"

if [[ -n "${CSV_PATH}" ]]; then
  if [[ "${DRY_RUN}" == "1" ]]; then
    echo "csv=${CSV_PATH} (dry-run; not writing)"
  else
    csv_header >"${CSV_PATH}"
    echo "csv=${CSV_PATH}"
  fi
fi

for direction in ${DIRECTIONS}; do
  if [[ "${direction}" != "client-to-relay" && "${direction}" != "relay-to-client-batch-read" ]]; then
    echo "invalid carrier direction: ${direction}" >&2
    exit 2
  fi

  for frames in ${FRAMES_LIST}; do
    if ! [[ "${frames}" =~ ^[0-9]+$ ]] || [[ "${frames}" == "0" ]]; then
      echo "invalid frame count: ${frames}" >&2
      exit 2
    fi

    for payload_bytes in ${PAYLOAD_BYTES_LIST}; do
      if ! [[ "${payload_bytes}" =~ ^[0-9]+$ ]] || [[ "${payload_bytes}" == "0" ]]; then
        echo "invalid payload byte count: ${payload_bytes}" >&2
        exit 2
      fi

      echo "== direction=${direction} frames=${frames} payload_bytes=${payload_bytes} =="
      cmd=(cargo run --release --bin ktp-tunnel-bench -- \
        --direction "${direction}" \
        --runs "${RUNS}" \
        --frames "${frames}" \
        --payload-bytes "${payload_bytes}")

      if [[ "${DRY_RUN}" == "1" ]]; then
        printf 'dry_run:'
        printf ' %q' "${cmd[@]}"
        printf '\n'
      else
        output="$("${cmd[@]}")"
        printf '%s\n' "${output}"
        if [[ -n "${CSV_PATH}" ]]; then
          write_csv_row "${output}"
        fi
      fi
    done
  done
done
