# kelicloud-agent-rs Smoke Compatibility Notes

Status: cross-platform backend protocol smoke has run against a real kelicloud
backend and passed the data-plane checks: basic-info upload, report WebSocket
connection, report send, and database persistence. The repeatable
local-backend Linux smoke path has also passed against the current kelicloud
backend and latest prepared web bundle, covering CN connectivity config, script
exec, TCP ping, admin WebSSH terminal, and forced auto-discovery token
rotation with post-recovery control-plane actions.

## Smoke Entry Points

- Cross-platform backend data-plane: `cargo run --locked --bin backend-protocol-smoke`.
- Local Linux: `scripts/smoke-live.sh --mode live --duration 120`.
- Local real backend Linux: `scripts/smoke-local-backend.sh`.
- GitHub Actions: manually run the `Smoke` workflow.
- GitHub Actions real backend: the `Local Backend Smoke` workflow runs on pushes
  to `main` and can also be run manually.
- Required secret: `KELICLOUD_SMOKE_TOKEN`.
- Alternative required secret: `KELICLOUD_SMOKE_AUTO_DISCOVERY_KEY`.
- Optional secrets: `KELICLOUD_SMOKE_ENDPOINT`,
  `KELICLOUD_SMOKE_CF_ACCESS_CLIENT_ID`,
  `KELICLOUD_SMOKE_CF_ACCESS_CLIENT_SECRET`.
- Optional inputs: `endpoint`, `mode`, `duration`, `expect_success_log`,
  `custom_dns`, `insecure`, `require_summary_pass`.
- Smoke log summarizer: `cargo run --locked --bin smoke-summary -- <log-file>`.

## Dynamic Smoke Evidence

The backend protocol smoke helper exists for workstations that cannot execute
the Linux-only agent binary. It uses the same `ReqwestHttpTransport` and
`TungsteniteWebSocketTransport` implementations as the main agent, but feeds
deterministic Linux-like payloads instead of collecting host metrics.

Observed data-plane evidence from the first real-backend run:

- `smoke: basic_info_uploaded`
- `smoke: report_websocket_connected`
- `smoke: report_sent`
- `agent loop: completed`

The backend database also persisted the smoke client as `linux/amd64` and wrote
the latest report row with CPU, TCP/UDP connection, and network counters. This
proves the current HTTP/WebSocket payload shape is accepted by the real backend.

Missing evidence from that run is expected because no live panel action was
triggered: ping task result upload, exec task result upload, terminal session,
and CN connectivity config receipt. `scripts/smoke-local-backend.sh` now covers
those automatically by starting kelicloud, creating a smoke client, enabling CN
connectivity settings, dispatching an exec task, creating a TCP ping task, and
opening an admin WebSSH terminal through `admin-terminal-smoke`.

First full local-backend control-plane pass:

- Commit: `172c1dc3cd5c966447e52781f84a26f266e0912c`
- Workflow: `Local Backend Smoke`
- Run: https://github.com/keli-123456/kelicloud-agent-rs/actions/runs/27487689850
- Evidence covered: basic-info upload, report WebSocket, repeated report sends,
  CN connectivity config receipt, exec task result upload, TCP ping result
  upload, admin terminal session start, xterm-compatible terminal input, and a
  live-agent duration marker for the long-running report loop.

First full auto-discovery forced-token-rotation pass:

- Commit: `a7fc75dd55e2863c800068d15dba2b9119cacddf`
- Workflow: `Local Backend Smoke`
- Run: https://github.com/keli-123456/kelicloud-agent-rs/actions/runs/27489107929
- CI run for the same commit:
  https://github.com/keli-123456/kelicloud-agent-rs/actions/runs/27489107928
- Evidence covered: startup auto-discovery registration, admin API token
  rotation for the initially registered client, invalid-token detection during
  periodic basic-info upload, second auto-discovery registration, report
  WebSocket reconnect with the recovered token, and post-recovery CN
  connectivity config, exec task result upload, TCP ping result upload, admin
  WebSSH terminal session, and live-agent duration evidence.
- Caveat: this smoke verifies token recovery and post-recovery control-plane
  behavior. Client deletion/offline cleanup behavior is still outside this
  smoke path.

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
  cycle, again after a successful report send, and during the report wait in
  one-second slices, so queued exec, ping, and terminal requests do not have to
  wait behind the next report payload.
- Basic-info upload and report WebSocket connection classify Go-agent-style
  HTTP 401 invalid-token responses as typed invalid-token transport errors,
  preserving the operation name, token, status code, and response body for
  auto-discovery recovery logic.
- Startup auto-discovery supports `--auto-discovery` /
  `AGENT_AUTO_DISCOVERY_KEY`, loads `auto-discovery.json` from the executable
  directory when present, otherwise registers at
  `/api/clients/register?name=<hostname>` with `{"key":"..."}`,
  `Authorization: Bearer <auto-discovery-key>`, Cloudflare Access headers when
  configured, and saves the returned `{uuid, token}` for normal report traffic.
- When auto-discovery is enabled, stale-token errors during basic-info upload or
  report WebSocket connection clear `auto-discovery.json`, re-register, update
  the in-memory token, rebuild the failed URL, and retry once with the fresh
  token. If the failed token differs from the current in-memory token, recovery
  treats it as already rotated, matching the Go agent guard.
- Task result upload and terminal connectors read a shared token at execution
  time, so auto-discovery token recovery is propagated to later exec result
  uploads and WebSSH terminal connection attempts.
- Cloudflare Access headers are supported for basic info, report WebSocket,
  terminal WebSocket, and task result upload.
- The live smoke path emits non-secret `smoke:` milestone lines for basic-info
  upload, report WebSocket connection, report sends, ping result upload, task
  result upload, terminal session start, and CN connectivity config receipt.
  `scripts/smoke-live.sh` turns those logs into a Markdown `*.summary.md`
  compatibility summary. Use `--require-summary-pass` only for runs where the
  panel actions are intentionally triggered; it fails the smoke when evidence is
  missing or failed.
- The local backend smoke path clones the backend, prepares the current web
  bundle through `scripts/prepare-frontend.sh`, starts a MySQL-backed kelicloud
  server, starts the agent through backend auto-discovery, rotates the
  auto-discovered token through the real admin edit endpoint, waits for
  invalid-token recovery and re-registration evidence, then triggers
  exec/ping/terminal/CN actions through real admin APIs against the recovered
  client and runs `smoke-summary --require-pass`. Its companion `Local Backend
  Smoke` workflow provides the Linux host that this Windows workstation lacks.
- Admin WebSSH terminal smoke must match browser behavior: include an `Origin`
  header accepted by backend `CheckOrigin`, wait until the backend/PTY sends a
  shell prompt before typing, send xterm input as binary bytes, and translate
  carriage return input to newline before writing to the Linux PTY.
- The local-backend smoke script opens `agent.log` in append mode after an
  explicit truncation. This prevents helper-written evidence, such as
  `live smoke duration reached`, from being overwritten by the still-running
  agent process.

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
11. Review the generated `*.summary.md`; any "missing" control-plane evidence
    should become the first compatibility gap to reproduce against the Go agent.
    For a full manual control-plane pass, run with `--require-summary-pass` so
    missing evidence fails the smoke immediately.

## First Compatibility Watchlist

These are not proven bugs, but they are the first places to inspect when the
dynamic smoke produces logs:

1. WebSocket read loop responsiveness

   The Go agent reads backend control messages in a dedicated goroutine while
   reports are sent by ticker. The Rust agent now drains buffered control
   messages before and after report sends, and polls them during the report
   wait in one-second slices. It is still not a dedicated read goroutine, so
   live smoke should verify that exec, ping, and terminal requests feel
   responsive with the production report interval and during reconnect or
   send-failure periods.

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

   The Rust prototype now supports the startup registration/cache path above,
   stale-token recovery for basic-info upload and report WebSocket connection,
   and shared-token propagation to task result upload and terminal connection
   attempts. Live smoke should still verify recovery with a real backend,
   especially with an exec task and a WebSSH session after a forced token
   rotation.

3. Auto-update

   The Go agent checks for updates unless disabled. The Rust prototype does not
   implement auto-update. This is intentionally outside the first smoke path,
   but it matters before replacement rollout.

4. Non-systemd installation

   The Go installer supports multiple init systems. The Rust installer currently
   targets systemd only. Runtime smoke can still pass, but installation parity is
   incomplete for OpenRC/procd/upstart hosts.

## Remaining Limits

- This workstation is Windows-only for this project, while the agent runtime is
  intentionally Linux-only. Use the `Local Backend Smoke` workflow for the
  repeatable full Linux control-plane check from here.
- Client deletion/offline cleanup after auto-discovery re-registration is not
  covered by the smoke path.
- Auto-update and non-systemd install parity remain outside the first
  replacement smoke path.
