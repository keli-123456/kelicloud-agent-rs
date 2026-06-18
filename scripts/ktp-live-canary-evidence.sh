#!/usr/bin/env bash
set -euo pipefail

SERVICE_NAME="kelicloud-agent-rs"
SINCE="30 minutes ago"
LOG_FILE=""
EVIDENCE_FILE="ktp-live-canary.evidence.md"
MIN_LINES=1

usage() {
    cat <<'USAGE'
KTP live tunnel diagnostics evidence collector.

Usage:
  scripts/ktp-live-canary-evidence.sh [options]

Options:
  --service-name NAME       systemd unit to read with journalctl
  --since VALUE             journalctl --since value, default: 30 minutes ago
  --log-file PATH           read an existing agent log instead of journalctl
  --evidence-file PATH      Markdown output, default: ktp-live-canary.evidence.md
  --min-lines N             minimum tunnel data diagnostics lines, default: 1
  -h, --help                show this help

The script validates that live KTP tunnel data diagnostics include runtime wait,
outbound queue dwell, and socket batch-read fields, then writes a small evidence
file.
USAGE
}

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

require_positive_integer() {
    local name="$1"
    local value="$2"
    [[ "$value" =~ ^[1-9][0-9]*$ ]] || die "${name} must be a positive integer"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --service-name)
            [[ $# -ge 2 ]] || die "--service-name requires a value"
            SERVICE_NAME="$2"
            shift 2
            ;;
        --since)
            [[ $# -ge 2 ]] || die "--since requires a value"
            SINCE="$2"
            shift 2
            ;;
        --log-file)
            [[ $# -ge 2 ]] || die "--log-file requires a value"
            LOG_FILE="$2"
            shift 2
            ;;
        --evidence-file)
            [[ $# -ge 2 ]] || die "--evidence-file requires a value"
            EVIDENCE_FILE="$2"
            shift 2
            ;;
        --min-lines)
            [[ $# -ge 2 ]] || die "--min-lines requires a value"
            MIN_LINES="$2"
            require_positive_integer "--min-lines" "$MIN_LINES"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            die "unknown argument: $1"
            ;;
    esac
done

require_positive_integer "--min-lines" "$MIN_LINES"

WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT
RAW_LOG="${WORKDIR}/agent.log"
DIAGNOSTICS_LOG="${WORKDIR}/ktp-diagnostics.log"

if [[ -n "$LOG_FILE" ]]; then
    [[ -f "$LOG_FILE" ]] || die "log file not found: $LOG_FILE"
    cp "$LOG_FILE" "$RAW_LOG"
else
    [[ "$(uname -s)" == "Linux" ]] || die "journal collection requires Linux; use --log-file elsewhere"
    command -v journalctl >/dev/null 2>&1 || die "journalctl not found; use --log-file"
    journalctl -u "$SERVICE_NAME" --since "$SINCE" --no-pager >"$RAW_LOG"
fi

grep -F "tunnel data diagnostics" "$RAW_LOG" >"$DIAGNOSTICS_LOG" || true
LINE_COUNT="$(wc -l <"$DIAGNOSTICS_LOG" | tr -d ' ')"
if (( LINE_COUNT < MIN_LINES )); then
    die "expected at least ${MIN_LINES} tunnel data diagnostics lines, found ${LINE_COUNT}"
fi

REQUIRED_FIELDS=(
    "runtime_wait_elapsed_p50_micros"
    "runtime_wait_elapsed_p95_micros"
    "runtime_wait_elapsed_p99_micros"
    "outbound_queue_dwell_p50_micros"
    "outbound_queue_dwell_p95_micros"
    "outbound_queue_dwell_p99_micros"
    "socket_read_batches"
    "socket_read_frames"
    "socket_read_max_batch_frames"
)

for field in "${REQUIRED_FIELDS[@]}"; do
    grep -F "${field}=" "$DIAGNOSTICS_LOG" >/dev/null || die "missing diagnostics field: ${field}"
done

mkdir -p "$(dirname "$EVIDENCE_FILE")"
{
    printf '# KTP Live Canary Evidence\n\n'
    printf -- '- Generated: `%s`\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    if [[ -n "$LOG_FILE" ]]; then
        printf -- '- Source: log file `%s`\n' "$LOG_FILE"
    else
        printf -- '- Source: journalctl unit `%s`\n' "$SERVICE_NAME"
        printf -- '- Since: `%s`\n' "$SINCE"
    fi
    printf -- '- Diagnostics lines: `%s`\n\n' "$LINE_COUNT"
    printf '## Required Fields\n\n'
    for field in "${REQUIRED_FIELDS[@]}"; do
        printf -- '- `%s`\n' "$field"
    done
    printf '\n## Latest Diagnostics\n\n'
    printf '```text\n'
    tail -n 5 "$DIAGNOSTICS_LOG"
    printf '\n```\n'
} >"$EVIDENCE_FILE"

printf 'ktp live canary evidence written: %s\n' "$EVIDENCE_FILE"
