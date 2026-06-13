# kelicloud-agent-rs Smoke Compatibility Notes

Status: dynamic smoke has not run yet. GitHub recognizes the `Smoke` workflow,
but the public workflow run list currently has no Smoke runs. A real run still
needs `KELICLOUD_SMOKE_TOKEN` and either the `endpoint` workflow input or
`KELICLOUD_SMOKE_ENDPOINT`.

## Smoke Entry Points

- Local Linux: `scripts/smoke-live.sh --mode live --duration 120`.
- GitHub Actions: manually run the `Smoke` workflow.
- Required secret: `KELICLOUD_SMOKE_TOKEN`.
- Optional secrets: `KELICLOUD_SMOKE_ENDPOINT`,
  `KELICLOUD_SMOKE_CF_ACCESS_CLIENT_ID`,
  `KELICLOUD_SMOKE_CF_ACCESS_CLIENT_SECRET`.
- Optional inputs: `endpoint`, `mode`, `duration`, `expect_success_log`,
  `custom_dns`, `insecure`.

## Static Parity Evidence

These areas have direct Rust tests or code paths matching the Go agent behavior:

- Basic info upload goes to `/api/clients/uploadBasicInfo?token=...` and retries
  without `kernel_version` for older backends.
- Report WebSocket goes to `/api/clients/report?token=...`.
- Report payload includes CPU, RAM, swap, load, disk, network totals/rates,
  uptime, process count, TCP/UDP connection count, optional GPU, optional
  `cn_connectivity`, and `message`.
- Remote exec handles empty command, disabled remote control, shebang scripts,
  `bash -lc` fallback, stdout/stderr combination, exit code, and task result
  upload to `/api/clients/task/result?token=...`.
- Ping control messages upload `ping_result` with `task_id`, `ping_type`,
  `value`, and `finished_at`.
- Ping high-latency behavior matches the Go agent: a first successful
  measurement above 1000 ms is retried up to three times; a later measurement at
  or below 1000 ms is reported, while repeated high latency or retry failure is
  reported as `-1`.
- Terminal control messages open `/api/clients/terminal?token=...&id=...`,
  create a Linux PTY, support input and resize messages, and send terminal
  output back over WebSocket.
- Report and terminal WebSocket URLs convert IDN hostnames to ASCII/Punycode,
  matching the Go agent's `ConvertIDNToASCII` behavior.
- The report loop drains buffered backend control messages at the start of each
  cycle and again after a successful report send, so queued exec, ping, and
  terminal requests do not have to wait behind the next report payload.
- Cloudflare Access headers are supported for basic info, report WebSocket,
  terminal WebSocket, and task result upload.

## First Dynamic Smoke Checks

Run these during the first `live` smoke:

1. Server appears in the panel with IPv4/IPv6, version, OS, kernel, memory,
   swap, disk, virtualization, and GPU name values that look sane.
2. Report table refreshes CPU/RAM/disk/network/process/TCP/UDP values at the
   configured interval.
3. A TCP ping task returns a non-negative latency or `-1` for failure.
4. An HTTP ping task returns the expected latency/failure shape.
5. An ICMP ping task works on the runner/host or fails as `-1` when ICMP is not
   permitted.
6. A script exec task such as `whoami` uploads output and exit code.
7. A shebang script task uploads output and exit code.
8. Terminal opens, echoes input, resizes, and closes cleanly.
9. `cn_connectivity_probe_config` updates the report field after one probe
   cycle.
10. If Cloudflare Access protects the panel, all HTTP and WebSocket paths work
    with the configured CF Access secrets.

## First Compatibility Watchlist

These are not proven bugs, but they are the first places to inspect when the
dynamic smoke produces logs:

1. WebSocket read loop responsiveness

   The Go agent reads backend control messages in a dedicated goroutine while
   reports are sent by ticker. The Rust agent now drains buffered control
   messages both before and after report sends, which removes the most visible
   "queued behind report" delay for messages already waiting on the socket. It
   is still not a dedicated read goroutine, so live smoke should verify that
   exec, ping, and terminal requests feel responsive with the production report
   interval and during reconnect or send-failure periods.

2. Auto-discovery and token recovery

   The Go agent supports `--auto-discovery` / `AGENT_AUTO_DISCOVERY_KEY`. On
   startup it loads `auto-discovery.json` from the executable directory when
   present; otherwise it registers with
   `POST /api/clients/register?name=<hostname>`, sends `{"key":"..."}`, sets
   `Authorization: Bearer <auto-discovery-key>`, includes Cloudflare Access
   headers when configured, stores the returned `{uuid, token}`, and then uses
   that token for normal report traffic.

   The Go agent also classifies HTTP 401 responses whose body mentions
   `invalid token`, `token is required`, or `failed to validate token` during
   basic-info upload and report WebSocket connection. When auto-discovery is
   enabled, it clears the cached `auto-discovery.json`, re-registers, and
   retries with the new token. If another thread has already rotated the token,
   it treats the stale-token error as recovered.

   The Rust prototype currently requires a fixed endpoint and token. It ignores
   Go-only flags such as `--auto-discovery`, has no `AGENT_AUTO_DISCOVERY_KEY`
   field, does not persist `auto-discovery.json`, and surfaces 401 responses as
   generic transport failures rather than typed invalid-token errors. This is
   not a blocker for a static-token smoke test, but it is a deployment
   compatibility gap if production relies on auto-discovery or stale-token
   recovery.

3. Auto-update

   The Go agent checks for updates unless disabled. The Rust prototype does not
   implement auto-update. This is intentionally outside the first smoke path,
   but it matters before replacement rollout.

4. Non-systemd installation

   The Go installer supports multiple init systems. The Rust installer currently
   targets systemd only. Runtime smoke can still pass, but installation parity is
   incomplete for OpenRC/procd/upstart hosts.

## Current Blockers

- No local `AGENT_ENDPOINT` or `AGENT_TOKEN` is configured.
- `gh` CLI is not installed locally, so this environment cannot dispatch
  GitHub Actions workflows.
- No local WSL distribution or Docker Linux environment is installed.
- Public GitHub API shows `Smoke` workflow is active but has zero runs.
