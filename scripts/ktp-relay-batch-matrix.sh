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
RELAY_BATCH_POLICIES="${KTP_BATCH_MATRIX_BATCH_POLICIES:-${KTP_BATCH_MATRIX_BATCH_POLICY:-fixed}}"
RELAY_WAIT_TIMEOUT_US="${KTP_BATCH_MATRIX_RELAY_WAIT_TIMEOUT_US:-100}"
DRY_RUN="${KTP_BATCH_MATRIX_DRY_RUN:-0}"
CSV_PATH="${KTP_BATCH_MATRIX_CSV:-}"
FAIL_ON_FIXED_BETTER="${KTP_BATCH_MATRIX_FAIL_ON_FIXED_BETTER:-0}"
MAX_ADAPTIVE_RTT_P95_MICROS="${KTP_BATCH_MATRIX_MAX_ADAPTIVE_RTT_P95_MICROS:-}"
MAX_ADAPTIVE_CLIENT_P95_SPREAD_MICROS="${KTP_BATCH_MATRIX_MAX_ADAPTIVE_CLIENT_P95_SPREAD_MICROS:-}"

csv_header() {
  printf '%s\n' "profile,runs,clients,frames,payload_bytes,client_payload_reused,relay_batch_frames,relay_batch_policy,relay_batch_frames_effective,elapsed_ms_min,elapsed_ms_median,elapsed_ms_max,throughput_mib_s_min,throughput_mib_s_median,throughput_mib_s_max,rtt_micros_p50,rtt_micros_p95,rtt_micros_p99,rtt_micros_max,rtt_client_p95_micros_min,rtt_client_p95_micros_max,rtt_client_p95_spread_micros,rtt_client_max_micros_max,relay_turns,relay_wait_turns,ingress_batches,egress_batches,ingress_max_batch_frames,egress_max_batch_frames"
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
  local clients="$1"
  local batch="$2"
  local output="$3"
  local client_payload_reused
  local elapsed_ms_min elapsed_ms_median elapsed_ms_max
  local throughput_mib_s_min throughput_mib_s_median throughput_mib_s_max
  local rtt_micros_p50 rtt_micros_p95 rtt_micros_p99 rtt_micros_max
  local rtt_client_p95_micros_min rtt_client_p95_micros_max
  local rtt_client_p95_spread_micros rtt_client_max_micros_max
  local relay_batch_policy relay_batch_frames_effective
  local relay_turns relay_wait_turns ingress_batches egress_batches
  local ingress_max_batch_frames egress_max_batch_frames

  client_payload_reused="$(required_metric_value "${output}" client_payload_reused)"
  relay_batch_policy="$(required_metric_value "${output}" relay_batch_policy)"
  relay_batch_frames_effective="$(required_metric_value "${output}" relay_batch_frames_effective relay_batch_frames)"
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
  rtt_client_p95_micros_min="$(required_metric_value "${output}" rtt_client_p95_micros_min)"
  rtt_client_p95_micros_max="$(required_metric_value "${output}" rtt_client_p95_micros_max)"
  rtt_client_p95_spread_micros="$(required_metric_value "${output}" rtt_client_p95_spread_micros)"
  rtt_client_max_micros_max="$(required_metric_value "${output}" rtt_client_max_micros_max)"
  relay_turns="$(required_metric_value "${output}" relay_turns)"
  relay_wait_turns="$(required_metric_value "${output}" relay_wait_turns)"
  ingress_batches="$(required_metric_value "${output}" ingress_batches)"
  egress_batches="$(required_metric_value "${output}" egress_batches)"
  ingress_max_batch_frames="$(required_metric_value "${output}" ingress_max_batch_frames)"
  egress_max_batch_frames="$(required_metric_value "${output}" egress_max_batch_frames)"

  printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
    "${PROFILE}" \
    "${RUNS}" \
    "${clients}" \
    "${FRAMES}" \
    "${PAYLOAD_BYTES}" \
    "${client_payload_reused}" \
    "${batch}" \
    "${relay_batch_policy}" \
    "${relay_batch_frames_effective}" \
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
    "${rtt_client_p95_micros_min}" \
    "${rtt_client_p95_micros_max}" \
    "${rtt_client_p95_spread_micros}" \
    "${rtt_client_max_micros_max}" \
    "${relay_turns}" \
    "${relay_wait_turns}" \
    "${ingress_batches}" \
    "${egress_batches}" \
    "${ingress_max_batch_frames}" \
    "${egress_max_batch_frames}" >>"${CSV_PATH}"
}

echo "== ktp relay batch matrix =="
echo "profile=${PROFILE} runs=${RUNS} clients=${CLIENTS} frames=${FRAMES} payload_bytes=${PAYLOAD_BYTES} relay_batch_policies=${RELAY_BATCH_POLICIES} relay_wait_timeout_us=${RELAY_WAIT_TIMEOUT_US}"
echo "batches=${BATCHES}"

POLICY_GATE_ENABLED=0
if [[ "${FAIL_ON_FIXED_BETTER}" == "1" || -n "${MAX_ADAPTIVE_RTT_P95_MICROS}" || -n "${MAX_ADAPTIVE_CLIENT_P95_SPREAD_MICROS}" ]]; then
  POLICY_GATE_ENABLED=1
fi

if [[ "${POLICY_GATE_ENABLED}" == "1" && "${DRY_RUN}" != "1" && -z "${CSV_PATH}" ]]; then
  echo "KTP batch matrix policy gates require KTP_BATCH_MATRIX_CSV" >&2
  exit 2
fi

if [[ -n "${CSV_PATH}" ]]; then
  if [[ "${DRY_RUN}" == "1" ]]; then
    echo "csv=${CSV_PATH} (dry-run; not writing)"
  else
    csv_header >"${CSV_PATH}"
    echo "csv=${CSV_PATH}"
  fi
fi

for policy in ${RELAY_BATCH_POLICIES}; do
  if [[ "${policy}" != "fixed" && "${policy}" != "adaptive" ]]; then
    echo "invalid relay batch policy: ${policy}" >&2
    exit 2
  fi

  for clients in ${CLIENTS}; do
    if ! [[ "${clients}" =~ ^[0-9]+$ ]] || [[ "${clients}" == "0" ]]; then
      echo "invalid client count: ${clients}" >&2
      exit 2
    fi

    for batch in ${BATCHES}; do
      if ! [[ "${batch}" =~ ^[0-9]+$ ]] || [[ "${batch}" == "0" ]]; then
        echo "invalid relay batch frame count: ${batch}" >&2
        exit 2
      fi

      echo "== relay_batch_policy=${policy} clients=${clients} relay_batch_frames=$batch =="
      cmd=(cargo run --release --bin ktp-e2e-bench -- \
        --profile "${PROFILE}" \
        --diagnostics \
        --latency \
        --relay-wait-timeout-us "${RELAY_WAIT_TIMEOUT_US}" \
        --runs "${RUNS}" \
        --clients "${clients}" \
        --frames "${FRAMES}" \
        --payload-bytes "${PAYLOAD_BYTES}" \
        --relay-batch-policy "${policy}" \
        --relay-batch-frames "${batch}")

      if [[ "${DRY_RUN}" == "1" ]]; then
        printf 'dry_run:'
        printf ' %q' "${cmd[@]}"
        printf '\n'
      else
        output="$("${cmd[@]}")"
        printf '%s\n' "${output}"
        if [[ -n "${CSV_PATH}" ]]; then
          write_csv_row "${clients}" "${batch}" "${output}"
        fi
      fi
    done
  done
done

if [[ "${POLICY_GATE_ENABLED}" == "1" ]]; then
  if [[ "${DRY_RUN}" == "1" ]]; then
    echo "ktp policy summary gate skipped in dry-run"
  else
    echo "== ktp policy summary gate =="
    summary_cmd=(cargo run --release --bin ktp-policy-summary --)
    if [[ "${FAIL_ON_FIXED_BETTER}" == "1" ]]; then
      summary_cmd+=(--fail-on-fixed-better)
    fi
    if [[ -n "${MAX_ADAPTIVE_RTT_P95_MICROS}" ]]; then
      summary_cmd+=(--max-adaptive-rtt-p95-micros "${MAX_ADAPTIVE_RTT_P95_MICROS}")
    fi
    if [[ -n "${MAX_ADAPTIVE_CLIENT_P95_SPREAD_MICROS}" ]]; then
      summary_cmd+=(--max-adaptive-client-p95-spread-micros "${MAX_ADAPTIVE_CLIENT_P95_SPREAD_MICROS}")
    fi
    summary_cmd+=("${CSV_PATH}")
    "${summary_cmd[@]}"
  fi
fi
