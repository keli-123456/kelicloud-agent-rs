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

echo "== ktp relay batch matrix =="
echo "profile=${PROFILE} runs=${RUNS} clients=${CLIENTS} frames=${FRAMES} payload_bytes=${PAYLOAD_BYTES} relay_wait_timeout_us=${RELAY_WAIT_TIMEOUT_US}"
echo "batches=${BATCHES}"

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
    "${cmd[@]}"
  fi
done
