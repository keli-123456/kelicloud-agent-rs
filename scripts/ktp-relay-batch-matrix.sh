#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "ktp relay batch matrix is Linux-only" >&2
  exit 2
fi

BATCHES="${KTP_BATCH_MATRIX_BATCHES:-1 2 4 8 16 32 64}"
RUNS="${KTP_BATCH_MATRIX_RUNS:-3}"
CLIENTS="${KTP_BATCH_MATRIX_CLIENTS:-2}"
FRAMES="${KTP_BATCH_MATRIX_FRAMES:-64}"
PAYLOAD_BYTES="${KTP_BATCH_MATRIX_PAYLOAD_BYTES:-8192}"
PROFILE="${KTP_BATCH_MATRIX_PROFILE:-rdp-like}"
RELAY_WAIT_TIMEOUT_US="${KTP_BATCH_MATRIX_RELAY_WAIT_TIMEOUT_US:-100}"
DRY_RUN="${KTP_BATCH_MATRIX_DRY_RUN:-0}"
CSV_PATH="${KTP_BATCH_MATRIX_CSV:-}"

csv_header() {
  printf '%s\n' "profile,runs,clients,frames,payload_bytes,relay_batch_frames,elapsed_ms_min,elapsed_ms_median,elapsed_ms_max,throughput_mib_s_min,throughput_mib_s_median,throughput_mib_s_max,rtt_micros_p50,rtt_micros_p95,rtt_micros_p99,rtt_micros_max,relay_turns,relay_wait_turns,ingress_batches,egress_batches,ingress_max_batch_frames,egress_max_batch_frames"
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
    echo "missing metric in ktp-e2e-bench output: $*" >&2
    return 1
  fi
  printf '%s\n' "${value}"
}

write_csv_row() {
  local batch="$1"
  local output="$2"
  local elapsed_ms_min elapsed_ms_median elapsed_ms_max
  local throughput_mib_s_min throughput_mib_s_median throughput_mib_s_max
  local rtt_micros_p50 rtt_micros_p95 rtt_micros_p99 rtt_micros_max
  local relay_turns relay_wait_turns ingress_batches egress_batches
  local ingress_max_batch_frames egress_max_batch_frames

  elapsed_ms_min="$(required_metric_value "${output}" elapsed_ms_min elapsed_ms)"
  elapsed_ms_median="$(required_metric_value "${output}" elapsed_ms_median elapsed_ms)"
  elapsed_ms_max="$(required_metric_value "${output}" elapsed_ms_max elapsed_ms)"
  throughput_mib_s_min="$(required_metric_value "${output}" throughput_mib_s_min throughput_mib_s)"
  throughput_mib_s_median="$(required_metric_value "${output}" throughput_mib_s_median throughput_mib_s)"
  throughput_mib_s_max="$(required_metric_value "${output}" throughput_mib_s_max throughput_mib_s)"
  rtt_micros_p50="$(required_metric_value "${output}" rtt_micros_p50)"
  rtt_micros_p95="$(required_metric_value "${output}" rtt_micros_p95)"
  rtt_micros_p99="$(required_metric_value "${output}" rtt_micros_p99)"
  rtt_micros_max="$(required_metric_value "${output}" rtt_micros_max)"
  relay_turns="$(required_metric_value "${output}" relay_turns)"
  relay_wait_turns="$(required_metric_value "${output}" relay_wait_turns)"
  ingress_batches="$(required_metric_value "${output}" ingress_batches)"
  egress_batches="$(required_metric_value "${output}" egress_batches)"
  ingress_max_batch_frames="$(required_metric_value "${output}" ingress_max_batch_frames)"
  egress_max_batch_frames="$(required_metric_value "${output}" egress_max_batch_frames)"

  printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
    "${PROFILE}" \
    "${RUNS}" \
    "${CLIENTS}" \
    "${FRAMES}" \
    "${PAYLOAD_BYTES}" \
    "${batch}" \
    "${elapsed_ms_min}" \
    "${elapsed_ms_median}" \
    "${elapsed_ms_max}" \
    "${throughput_mib_s_min}" \
    "${throughput_mib_s_median}" \
    "${throughput_mib_s_max}" \
    "${rtt_micros_p50}" \
    "${rtt_micros_p95}" \
    "${rtt_micros_p99}" \
    "${rtt_micros_max}" \
    "${relay_turns}" \
    "${relay_wait_turns}" \
    "${ingress_batches}" \
    "${egress_batches}" \
    "${ingress_max_batch_frames}" \
    "${egress_max_batch_frames}" >>"${CSV_PATH}"
}

echo "== ktp relay batch matrix =="
echo "profile=${PROFILE} runs=${RUNS} clients=${CLIENTS} frames=${FRAMES} payload_bytes=${PAYLOAD_BYTES} relay_wait_timeout_us=${RELAY_WAIT_TIMEOUT_US}"
echo "batches=${BATCHES}"

if [[ -n "${CSV_PATH}" ]]; then
  if [[ "${DRY_RUN}" == "1" ]]; then
    echo "csv=${CSV_PATH} (dry-run; not writing)"
  else
    csv_header >"${CSV_PATH}"
    echo "csv=${CSV_PATH}"
  fi
fi

for batch in ${BATCHES}; do
  if ! [[ "${batch}" =~ ^[0-9]+$ ]] || [[ "${batch}" == "0" ]]; then
    echo "invalid relay batch frame count: ${batch}" >&2
    exit 2
  fi

  echo "== relay_batch_frames=$batch =="
  cmd=(cargo run --release --bin ktp-e2e-bench -- \
    --profile "${PROFILE}" \
    --diagnostics \
    --latency \
    --relay-wait-timeout-us "${RELAY_WAIT_TIMEOUT_US}" \
    --runs "${RUNS}" \
    --clients "${CLIENTS}" \
    --frames "${FRAMES}" \
    --payload-bytes "${PAYLOAD_BYTES}" \
    --relay-batch-frames "${batch}")

  if [[ "${DRY_RUN}" == "1" ]]; then
    printf 'dry_run:'
    printf ' %q' "${cmd[@]}"
    printf '\n'
  else
    output="$("${cmd[@]}")"
    printf '%s\n' "${output}"
    if [[ -n "${CSV_PATH}" ]]; then
      write_csv_row "${batch}" "${output}"
    fi
  fi
done
