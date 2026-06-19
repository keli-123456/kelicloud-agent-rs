#!/usr/bin/env bash
set -euo pipefail

SERVICE_NAME="kelicloud-agent-rs"
SINCE="30 minutes ago"
LOG_FILE=""
EVIDENCE_FILE="ktp-live-canary.evidence.md"
MIN_LINES=1
TUNNEL_ECHO_EVIDENCE_FILE=""
MIN_MAX_BATCH_FRAMES="${KTP_LIVE_CANARY_MIN_MAX_BATCH_FRAMES:-1}"
MIN_MAX_WRITE_BATCH_FRAMES="${KTP_LIVE_CANARY_MIN_MAX_WRITE_BATCH_FRAMES:-1}"
AUTH_VERSION="${KTP_LIVE_CANARY_AUTH_VERSION:-v1}"
CARRIER="${KTP_LIVE_CANARY_CARRIER:-ktp_tcp}"

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
  --tunnel-echo-evidence-file PATH
                            include and validate tunnel-echo.evidence.md from
                            the same canary observation window
  -h, --help                show this help

Environment:
  KTP_LIVE_CANARY_MIN_MAX_BATCH_FRAMES
                            require socket_read_max_batch_frames to reach this
                            value, default: 1
  KTP_LIVE_CANARY_MIN_MAX_WRITE_BATCH_FRAMES
                            require socket_write_max_batch_frames to reach this
                            value, default: 1
  KTP_LIVE_CANARY_AUTH_VERSION
                            require auth=ktp_token_preface_v1 or v2, default: v1
  KTP_LIVE_CANARY_CARRIER
                            require carrier=ktp_tcp or ktp_tls, default: ktp_tcp

The script validates that live KTP startup output declares the dedicated
carrier, self-managed crypto, token auth preface, and active relay batch
policy, that diagnostics include runtime wait, lifetime and recent outbound
queue dwell, and socket batch-read/write fields, then writes a small evidence
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
        --tunnel-echo-evidence-file)
            [[ $# -ge 2 ]] || die "--tunnel-echo-evidence-file requires a value"
            TUNNEL_ECHO_EVIDENCE_FILE="$2"
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
require_positive_integer "KTP_LIVE_CANARY_MIN_MAX_BATCH_FRAMES" "$MIN_MAX_BATCH_FRAMES"
require_positive_integer "KTP_LIVE_CANARY_MIN_MAX_WRITE_BATCH_FRAMES" "$MIN_MAX_WRITE_BATCH_FRAMES"
case "$AUTH_VERSION" in
    v1|v2) ;;
    *) die "KTP_LIVE_CANARY_AUTH_VERSION must be v1 or v2" ;;
esac
case "$CARRIER" in
    ktp_tcp|ktp_tls) ;;
    *) die "KTP_LIVE_CANARY_CARRIER must be ktp_tcp or ktp_tls" ;;
esac

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

require_startup_evidence() {
    local pattern="$1"
    local name="$2"
    grep -F "${pattern}" "$RAW_LOG" >/dev/null || die "missing startup evidence: ${name}"
}

startup_evidence_line() {
    local pattern="$1"
    grep -F "${pattern}" "$RAW_LOG" | tail -n 1
}

require_startup_evidence "tunnel data: enabled" "tunnel data enabled"
require_startup_evidence "carrier=${CARRIER}" "ktp carrier ${CARRIER}"
require_startup_evidence "crypto=ktp_aead" "ktp tcp crypto"
require_startup_evidence "auth=ktp_token_preface_${AUTH_VERSION}" "ktp tcp token auth preface"
require_startup_evidence "ktp relay batch policy:" "ktp relay batch policy"
require_startup_evidence "adaptive high_sessions=" "adaptive high_sessions"
require_startup_evidence "elevated_dwell_us=" "adaptive elevated_dwell_us"
require_startup_evidence "severe_dwell_us=" "adaptive severe_dwell_us"
require_startup_evidence "elevated_cap=" "adaptive elevated_cap"
require_startup_evidence "severe_cap=" "adaptive severe_cap"

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
    "recent_outbound_queue_dwell_p50_micros"
    "recent_outbound_queue_dwell_p95_micros"
    "recent_outbound_queue_dwell_p99_micros"
    "socket_read_batches"
    "socket_read_frames"
    "socket_read_max_batch_frames"
    "socket_write_batches"
    "socket_write_frames"
    "socket_write_max_batch_frames"
    "socket_write_batch_limit_max"
    "socket_write_batch_limit_min"
    "socket_write_batch_limit_last"
)

POSITIVE_FIELDS=(
    "socket_read_batches"
    "socket_read_frames"
    "socket_read_max_batch_frames"
    "socket_write_batches"
    "socket_write_frames"
    "socket_write_max_batch_frames"
    "socket_write_batch_limit_max"
    "socket_write_batch_limit_min"
    "socket_write_batch_limit_last"
)

max_metric_value() {
    local field="$1"
    awk -v field="${field}" '
        {
            for (i = 1; i <= NF; i++) {
                split($i, pair, "=")
                if (pair[1] == field && pair[2] ~ /^[0-9]+$/ && pair[2] + 0 > max) {
                    max = pair[2] + 0
                }
            }
        }
        END { print max + 0 }
    ' "$DIAGNOSTICS_LOG"
}

min_positive_metric_value() {
    local field="$1"
    awk -v field="${field}" '
        {
            for (i = 1; i <= NF; i++) {
                split($i, pair, "=")
                if (pair[1] == field && pair[2] ~ /^[0-9]+$/ && pair[2] + 0 > 0) {
                    if (min == "" || pair[2] + 0 < min) {
                        min = pair[2] + 0
                    }
                }
            }
        }
        END { print min == "" ? 0 : min }
    ' "$DIAGNOSTICS_LOG"
}

last_metric_value() {
    local field="$1"
    awk -v field="${field}" '
        {
            for (i = 1; i <= NF; i++) {
                split($i, pair, "=")
                if (pair[1] == field && pair[2] ~ /^[0-9]+$/) {
                    value = pair[2] + 0
                }
            }
        }
        END { print value + 0 }
    ' "$DIAGNOSTICS_LOG"
}

evidence_metric_value() {
    local field="$1"
    case "${field}" in
        socket_write_batch_limit_min)
            min_positive_metric_value "${field}"
            ;;
        socket_write_batch_limit_last)
            last_metric_value "${field}"
            ;;
        *)
            max_metric_value "${field}"
            ;;
    esac
}

markdown_field_value() {
    local file="$1"
    local field="$2"
    awk -v field="${field}" '
        {
            line = $0
            sub(/^[[:space:]]*-[[:space:]]*/, "", line)
            if (index(line, field ":") == 1) {
                sub("^[^:]*:[[:space:]]*", "", line)
                print line
                exit
            }
        }
    ' "$file"
}

require_markdown_field() {
    local file="$1"
    local field="$2"
    local value
    value="$(markdown_field_value "$file" "$field")"
    [[ -n "$value" ]] || die "missing tunnel echo evidence field: ${field}"
}

require_markdown_positive_integer() {
    local file="$1"
    local field="$2"
    local value
    value="$(markdown_field_value "$file" "$field")"
    [[ "$value" =~ ^[1-9][0-9]*$ ]] || die "tunnel echo evidence field ${field} must be a positive integer"
}

require_markdown_non_negative_integer() {
    local file="$1"
    local field="$2"
    local value
    value="$(markdown_field_value "$file" "$field")"
    [[ "$value" =~ ^[0-9]+$ ]] || die "tunnel echo evidence field ${field} must be a non-negative integer"
}

require_markdown_positive_decimal() {
    local file="$1"
    local field="$2"
    local value
    value="$(markdown_field_value "$file" "$field")"
    [[ "$value" =~ ^[0-9]+([.][0-9]+)?$ ]] || die "tunnel echo evidence field ${field} must be numeric"
    awk -v value="$value" 'BEGIN { exit(value + 0 > 0 ? 0 : 1) }' ||
        die "tunnel echo evidence field ${field} must be positive"
}

TUNNEL_ECHO_REQUIRED_FIELDS=(
    "profile"
    "rounds"
    "clients"
    "total_payload_bytes"
    "echo_elapsed_micros"
    "echo_throughput_mib_s"
    "rtt_micros_p50"
    "rtt_micros_p95"
    "rtt_micros_p99"
    "rtt_micros_max"
    "rtt_client_p95_spread_micros"
)

validate_tunnel_echo_evidence() {
    [[ -n "$TUNNEL_ECHO_EVIDENCE_FILE" ]] || return 0
    [[ -f "$TUNNEL_ECHO_EVIDENCE_FILE" ]] ||
        die "tunnel echo evidence file not found: ${TUNNEL_ECHO_EVIDENCE_FILE}"
    grep -F "# Tunnel Echo Evidence" "$TUNNEL_ECHO_EVIDENCE_FILE" >/dev/null ||
        die "tunnel echo evidence file is not a Tunnel Echo Evidence report"

    for field in "${TUNNEL_ECHO_REQUIRED_FIELDS[@]}"; do
        require_markdown_field "$TUNNEL_ECHO_EVIDENCE_FILE" "$field"
    done
    require_markdown_positive_integer "$TUNNEL_ECHO_EVIDENCE_FILE" "rounds"
    require_markdown_positive_integer "$TUNNEL_ECHO_EVIDENCE_FILE" "clients"
    require_markdown_positive_integer "$TUNNEL_ECHO_EVIDENCE_FILE" "total_payload_bytes"
    require_markdown_positive_integer "$TUNNEL_ECHO_EVIDENCE_FILE" "echo_elapsed_micros"
    require_markdown_positive_decimal "$TUNNEL_ECHO_EVIDENCE_FILE" "echo_throughput_mib_s"
    require_markdown_positive_integer "$TUNNEL_ECHO_EVIDENCE_FILE" "rtt_micros_p50"
    require_markdown_positive_integer "$TUNNEL_ECHO_EVIDENCE_FILE" "rtt_micros_p95"
    require_markdown_positive_integer "$TUNNEL_ECHO_EVIDENCE_FILE" "rtt_micros_p99"
    require_markdown_positive_integer "$TUNNEL_ECHO_EVIDENCE_FILE" "rtt_micros_max"
    require_markdown_non_negative_integer "$TUNNEL_ECHO_EVIDENCE_FILE" "rtt_client_p95_spread_micros"
}

for field in "${REQUIRED_FIELDS[@]}"; do
    grep -F "${field}=" "$DIAGNOSTICS_LOG" >/dev/null || die "missing diagnostics field: ${field}"
done

for field in "${POSITIVE_FIELDS[@]}"; do
    value="$(evidence_metric_value "${field}")"
    (( value > 0 )) || die "expected positive diagnostics field: ${field}"
done

max_batch_frames="$(max_metric_value "socket_read_max_batch_frames")"
if (( max_batch_frames < MIN_MAX_BATCH_FRAMES )); then
    die "expected socket_read_max_batch_frames >= ${MIN_MAX_BATCH_FRAMES}, found ${max_batch_frames}"
fi

max_write_batch_frames="$(max_metric_value "socket_write_max_batch_frames")"
if (( max_write_batch_frames < MIN_MAX_WRITE_BATCH_FRAMES )); then
    die "expected socket_write_max_batch_frames >= ${MIN_MAX_WRITE_BATCH_FRAMES}, found ${max_write_batch_frames}"
fi

validate_tunnel_echo_evidence

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
    printf '## Startup Policy\n\n'
    printf '```text\n'
    startup_evidence_line "tunnel data: enabled"
    startup_evidence_line "ktp relay batch policy:"
    startup_evidence_line "adaptive high_sessions="
    printf '\n```\n'
    printf '\n'
    printf '## Required Fields\n\n'
    for field in "${REQUIRED_FIELDS[@]}"; do
        printf -- '- `%s`\n' "$field"
    done
    printf '\n## Positive Fields\n\n'
    for field in "${POSITIVE_FIELDS[@]}"; do
        printf -- '- `%s`: `%s`\n' "$field" "$(evidence_metric_value "${field}")"
    done
    printf '\n## Batch Thresholds\n\n'
    printf -- '- `socket_read_max_batch_frames`: `%s`\n' "$max_batch_frames"
    printf -- '- `KTP_LIVE_CANARY_MIN_MAX_BATCH_FRAMES`: `%s`\n' "$MIN_MAX_BATCH_FRAMES"
    printf -- '- `socket_write_max_batch_frames`: `%s`\n' "$max_write_batch_frames"
    printf -- '- `KTP_LIVE_CANARY_MIN_MAX_WRITE_BATCH_FRAMES`: `%s`\n' "$MIN_MAX_WRITE_BATCH_FRAMES"
    if [[ -n "$TUNNEL_ECHO_EVIDENCE_FILE" ]]; then
        printf '\n## Tunnel Echo Evidence\n\n'
        printf -- '- Source: `%s`\n' "$TUNNEL_ECHO_EVIDENCE_FILE"
        for field in "${TUNNEL_ECHO_REQUIRED_FIELDS[@]}"; do
            printf -- '- %s: %s\n' "$field" "$(markdown_field_value "$TUNNEL_ECHO_EVIDENCE_FILE" "$field")"
        done
    fi
    printf '\n## Latest Diagnostics\n\n'
    printf '```text\n'
    tail -n 5 "$DIAGNOSTICS_LOG"
    printf '\n```\n'
} >"$EVIDENCE_FILE"

printf 'ktp live canary evidence written: %s\n' "$EVIDENCE_FILE"
