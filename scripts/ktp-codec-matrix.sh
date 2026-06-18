#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "ktp codec matrix is Linux-only" >&2
  exit 2
fi

MODES="${KTP_CODEC_MATRIX_MODES:-stream crypto}"
RUNS="${KTP_CODEC_MATRIX_RUNS:-3}"
FRAMES_LIST="${KTP_CODEC_MATRIX_FRAMES:-512 4096}"
PAYLOAD_BYTES_LIST="${KTP_CODEC_MATRIX_PAYLOAD_BYTES:-1024 4096 16384}"
CHUNK_FRAMES="${KTP_CODEC_MATRIX_CHUNK_FRAMES:-64}"
DRY_RUN="${KTP_CODEC_MATRIX_DRY_RUN:-0}"
CSV_PATH="${KTP_CODEC_MATRIX_CSV:-}"

csv_header() {
  printf '%s\n' "mode,runs,frames,payload_bytes,chunk_frames,bytes_per_run,total_bytes,cursor_compaction,elapsed_ms_min,elapsed_ms_median,elapsed_ms_max,throughput_mib_s_min,throughput_mib_s_median,throughput_mib_s_max"
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
    echo "missing metric in ktp-codec-bench output: $*" >&2
    return 1
  fi
  printf '%s\n' "${value}"
}

validate_positive_int() {
  local name="$1"
  local value="$2"
  if ! [[ "${value}" =~ ^[0-9]+$ ]] || [[ "${value}" == "0" ]]; then
    echo "invalid ${name}: ${value}" >&2
    exit 2
  fi
}

write_csv_row() {
  local output="$1"
  local mode frames payload_bytes chunk_frames
  local bytes_per_run total_bytes cursor_compaction
  local elapsed_ms_min elapsed_ms_median elapsed_ms_max
  local throughput_mib_s_min throughput_mib_s_median throughput_mib_s_max

  mode="$(required_metric_value "${output}" mode)"
  frames="$(required_metric_value "${output}" frames)"
  payload_bytes="$(required_metric_value "${output}" payload_bytes)"
  chunk_frames="$(required_metric_value "${output}" chunk_frames)"
  bytes_per_run="$(required_metric_value "${output}" bytes_per_run bytes)"
  total_bytes="$(required_metric_value "${output}" total_bytes bytes)"
  cursor_compaction="$(required_metric_value "${output}" cursor_compaction)"
  elapsed_ms_min="$(required_metric_value "${output}" elapsed_ms_min elapsed_ms)"
  elapsed_ms_median="$(required_metric_value "${output}" elapsed_ms_median elapsed_ms)"
  elapsed_ms_max="$(required_metric_value "${output}" elapsed_ms_max elapsed_ms)"
  throughput_mib_s_min="$(required_metric_value "${output}" throughput_mib_s_min throughput_mib_s)"
  throughput_mib_s_median="$(required_metric_value "${output}" throughput_mib_s_median throughput_mib_s)"
  throughput_mib_s_max="$(required_metric_value "${output}" throughput_mib_s_max throughput_mib_s)"

  printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
    "${mode}" \
    "${RUNS}" \
    "${frames}" \
    "${payload_bytes}" \
    "${chunk_frames}" \
    "${bytes_per_run}" \
    "${total_bytes}" \
    "${cursor_compaction}" \
    "${elapsed_ms_min}" \
    "${elapsed_ms_median}" \
    "${elapsed_ms_max}" \
    "${throughput_mib_s_min}" \
    "${throughput_mib_s_median}" \
    "${throughput_mib_s_max}" >>"${CSV_PATH}"
}

validate_positive_int "run count" "${RUNS}"
validate_positive_int "chunk frame count" "${CHUNK_FRAMES}"

echo "== ktp codec matrix =="
echo "modes=${MODES} runs=${RUNS} frames=${FRAMES_LIST} payload_bytes=${PAYLOAD_BYTES_LIST} chunk_frames=${CHUNK_FRAMES}"

if [[ -n "${CSV_PATH}" ]]; then
  if [[ "${DRY_RUN}" == "1" ]]; then
    echo "csv=${CSV_PATH} (dry-run; not writing)"
  else
    csv_header >"${CSV_PATH}"
    echo "csv=${CSV_PATH}"
  fi
fi

for mode in ${MODES}; do
  if [[ "${mode}" != "stream" && "${mode}" != "crypto" ]]; then
    echo "invalid codec mode: ${mode}" >&2
    exit 2
  fi

  for frames in ${FRAMES_LIST}; do
    validate_positive_int "frame count" "${frames}"

    for payload_bytes in ${PAYLOAD_BYTES_LIST}; do
      validate_positive_int "payload byte count" "${payload_bytes}"

      echo "== mode=${mode} frames=${frames} payload_bytes=${payload_bytes} chunk_frames=${CHUNK_FRAMES} =="
      cmd=(cargo run --release --bin ktp-codec-bench -- \
        --mode "${mode}" \
        --runs "${RUNS}" \
        --frames "${frames}" \
        --payload-bytes "${payload_bytes}" \
        --chunk-frames "${CHUNK_FRAMES}")

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
