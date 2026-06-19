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
- Real Linux host install/rollback: `scripts/canary-install.sh`.
- Real Linux host install/control/rollback:
  `scripts/real-host-control-canary.sh`.
- Real Linux host live panel control-plane trigger:
  `scripts/live-panel-control-smoke.sh`.
- Local real backend Linux: `scripts/smoke-local-backend.sh`.
- GitHub Actions: manually run the `Smoke` workflow.
- GitHub Actions real host canary: manually run the `Real Host Canary` workflow
  on a self-hosted Linux runner labelled `kelicloud-canary`.
- GitHub Actions real host control canary: manually run the same
  `Real Host Canary` workflow with `control_plane=true`, plus either
  `KELICLOUD_PANEL_COOKIE` or
  `KELICLOUD_PANEL_USERNAME`/`KELICLOUD_PANEL_PASSWORD` repository secrets.
  If `KELICLOUD_CANARY_AUTO_DISCOVERY_KEY` is not set, the workflow can derive
  the endpoint and auto-discovery key from the existing Go agent service when
  `derive_auto_discovery_from_old_service=true`.
- GitHub Actions real backend: the `Local Backend Smoke` workflow runs on pushes
  to `main` and can also be run manually.
- Required secret: `KELICLOUD_SMOKE_TOKEN`.
- Alternative required secret: `KELICLOUD_SMOKE_AUTO_DISCOVERY_KEY`.
- Real-host canary required secret: `KELICLOUD_CANARY_AUTO_DISCOVERY_KEY`.
- Real-host canary optional secrets: `KELICLOUD_CANARY_ENDPOINT`,
  `KELICLOUD_CANARY_ROLLBACK_COMMAND`.
- Real-host control canary optional secrets: `KELICLOUD_PANEL_COOKIE`,
  `KELICLOUD_PANEL_USERNAME`, `KELICLOUD_PANEL_PASSWORD`,
  `KELICLOUD_PANEL_PING_TARGET`.
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

## Linux Release Canary Checklist

Use this checklist before treating the Rust agent as the default Linux install
path from kelicloud Web or backend-generated auto-connect snippets.

- [x] GitHub release exists with `kelicloud-agent-rs-linux-amd64`,
  `kelicloud-agent-rs-linux-arm64`, and `kelicloud-agent-rs-linux-armv7`
  assets.
- [x] A real Linux host installs from the panel-compatible auto-discovery
  command:
  `wget -qO- https://raw.githubusercontent.com/keli-123456/kelicloud-agent-rs/refs/heads/main/install.sh | sudo bash -s -- -e <endpoint> --auto-discovery <key>`.
- [x] The installed service appears online in the panel with sane CPU, RAM,
  disk, network rate, TCP/UDP connection count, uptime, OS, and version fields.
- [x] A script exec task uploads stdout/stderr and exit code.
- [x] A TCP ping task returns a backend-visible result.
- [x] An admin WebSSH terminal opens, accepts input, and closes cleanly.
- [x] `systemctl restart kelicloud-agent-rs` reconnects without manual token
  repair.
- [x] Re-running the install command with `--install-version <tag>` upgrades or
  pins the expected release asset.
- [x] `sudo ./install.sh uninstall` removes the systemd service, binary, config,
  and auto-discovery cache when `--keep-config` is not used.
- [x] Rollback path is verified by uninstalling the Rust service and restoring
  the existing panel-generated Go `komari-agent.service`.
- [ ] Fresh Go-agent reinstall from a newly generated panel command after Rust
  uninstall is still useful before a broad replacement rollout.

2026-06-14 release rollout evidence:

- Commit: `11efece64ec48c842bcfd0f76504fa7059356f92`
- Release tag: `v0.1.0`
- Release workflow:
  https://github.com/keli-123456/kelicloud-agent-rs/actions/runs/27490452767
- Release assets verified through GitHub API: `kelicloud-agent-rs-linux-amd64`,
  `kelicloud-agent-rs-linux-arm64`, and `kelicloud-agent-rs-linux-armv7`.
- CI workflow for the same commit:
  https://github.com/keli-123456/kelicloud-agent-rs/actions/runs/27490275253
- Local Backend Smoke workflow for the same commit:
  https://github.com/keli-123456/kelicloud-agent-rs/actions/runs/27490275252

2026-06-14 real-host canary evidence:

- Commit: `430113c46f72b60cd601b22b3b5d880c205f7a9c`
- Fix committed before this run: `install.sh` now stops an existing
  `kelicloud-agent-rs.service` before replacing `/usr/local/bin/kelicloud-agent-rs`,
  preventing Linux `Text file busy` failures during same-version
  `--install-version` pin/upgrade checks.
- CI workflow for the same commit:
  https://github.com/keli-123456/kelicloud-agent-rs/actions/runs/27494076308
- Local Backend Smoke workflow for the same commit:
  https://github.com/keli-123456/kelicloud-agent-rs/actions/runs/27494076313
- Date: `2026-06-14`
- Host/provider/region: `vm57463.desivps.com` / `2.56.116.39`
- Distro/kernel/arch: `Debian GNU/Linux 12 (bookworm)` /
  `6.1.0-31-amd64` / `x86_64`
- Panel endpoint: `https://tanzhen2.huhu.icu`
- Rust release/version requested: `v0.1.0`
- Release asset: `kelicloud-agent-rs-linux-amd64`
- Install command source: `scripts/canary-install.sh` downloading the latest
  `install.sh` from `main` with panel-compatible `--endpoint` and
  `--auto-discovery` arguments.
- Evidence file on host:
  `/root/kelicloud-agent-rs-canary-20260614T091124Z/real-host-canary.evidence.md`
- Canary result: `passed`
- Pre-switch service state: `komari-agent.service` was `active/enabled`;
  `kelicloud-agent-rs.service` was `inactive`.
- Systemd service status after Rust install: `active`
- Config verification: `/etc/kelicloud-agent-rs/config.env` contained
  `AGENT_ENDPOINT` and redacted `AGENT_AUTO_DISCOVERY_KEY`.
- `systemctl restart kelicloud-agent-rs` reconnect result: `passed`
- Explicit install-version pin/upgrade result: `passed: v0.1.0`
- Rust uninstall result: `passed`
- Go-agent rollback command result: `passed`
- Final rollback status: `komari-agent.service` returned to `active/enabled`;
  `kelicloud-agent-rs.service` returned to `inactive`.
- Panel online and metrics evidence from the Rust journal:
  `smoke: auto_discovery_registered`, `smoke: basic_info_uploaded`,
  `smoke: report_websocket_connected`, and 80 `smoke: report_sent` lines during
  the observation window, with zero `error` keyword matches.
- Admin WebSSH terminal evidence from the Rust journal:
  `smoke: terminal_session_started`, 31 `terminal_input_received` lines, and
  22 `terminal_output_sent` lines during the observation window.
- Script exec task result: not observed on this real host; no
  `task_result_uploaded` line appeared during the window.
- TCP ping task result: not observed on this real host; no
  `ping_result_uploaded` line appeared during the window.
- Remaining rollout gap: run one more short real-host canary window and trigger
  a script exec task plus a TCP ping task from the panel while Rust is installed.

2026-06-14 real-host installer replacement follow-up:

- Commit: `b56a879893e5fe9289547267587ff023dde6ac97`
- Fix committed before this run: `install.sh` now downloads the release asset to
  a same-directory temporary file and then `mv -f`s it over
  `/usr/local/bin/kelicloud-agent-rs`. This avoids writing directly to an inode
  that may still be executing, even if systemd shutdown has a small race.
- CI workflow for the same commit:
  https://github.com/keli-123456/kelicloud-agent-rs/actions/runs/27494681820
- Local Backend Smoke workflow for the same commit:
  https://github.com/keli-123456/kelicloud-agent-rs/actions/runs/27494681822
- Host/provider/region: `vm57463.desivps.com` / `2.56.116.39`
- Distro/kernel/arch: `Debian GNU/Linux 12 (bookworm)` /
  `6.1.0-31-amd64` / `x86_64`
- Evidence file on host:
  `/root/kelicloud-agent-rs-canary-20260614T093621Z-control/real-host-canary.evidence.md`
- Canary result: `passed`
- Explicit install-version pin/upgrade result: `passed: v0.1.0`; the previous
  `Text file busy` failure did not recur with the atomic replacement installer.
- Panel online and metrics evidence from the Rust journal:
  `smoke: basic_info_uploaded`, `smoke: report_websocket_connected`, and 120
  `smoke: report_sent` lines during the observation window, with zero `error`
  keyword matches.
- Script exec task result: not observed on this real host; no
  `task_result_uploaded` line appeared during either the six-minute observation
  window or the later manual hold window.
- TCP ping task result: not observed on this real host; no
  `ping_result_uploaded` line appeared during either the six-minute observation
  window or the later manual hold window.
- Manual hold window:
  `/root/kelicloud-agent-rs-canary-20260614T094336Z-manual` left Rust active for
  panel interaction, produced `basic_info_uploaded`, `report_websocket_connected`,
  and 112 `report_sent` lines, then was rolled back to `komari-agent.service`.
- Final host state after rollback: `komari-agent.service` was `active/enabled`;
  `kelicloud-agent-rs.service` was `inactive`.
- Remaining rollout gap: an authenticated admin must trigger one script exec
  task and one TCP ping task against the real Rust-hosted client, or provide an
  authenticated session so the same `POST /api/admin/task/exec` and
  `POST /api/admin/ping/add` calls used by `scripts/smoke-local-backend.sh` can
  be executed against the live panel. `scripts/live-panel-control-smoke.sh`
  automates this once `KELICLOUD_PANEL_COOKIE`, `--endpoint`, `--client`, and
  `--ping-target` are provided while `kelicloud-agent-rs.service` is active on
  the real host.

2026-06-14 integrated real-host wrapper follow-up:

- Commit: `596450549507acad035954c3fb7d46170137f3fb`
- CI workflow:
  https://github.com/keli-123456/kelicloud-agent-rs/actions/runs/27495588232
- Local Backend Smoke workflow:
  https://github.com/keli-123456/kelicloud-agent-rs/actions/runs/27495588233
- Host/provider/region: `vm57463.desivps.com` / `2.56.116.39`
- Wrapper command source: latest
  `scripts/real-host-control-canary.sh` from `main`.
- Wrapper mode: `--skip-control`, because no authenticated live-panel admin
  cookie was available in this session.
- Evidence file on host:
  `/root/kelicloud-agent-rs-canary-20260614T101903Z-wrapper-skip/real-host-control-canary.evidence.md`
- Wrapper result: `passed`
- Rust client UUID parsed from journal:
  `ba3612ba-9b73-4271-842c-be05615d7a6a`
- Verified wrapper stages: latest script download, Rust install, service
  activation, systemd restart, explicit `v0.1.0` pin/reinstall with atomic
  binary replacement, UUID parsing from
  `smoke: auto_discovery_registered uuid=...`, Rust uninstall, and old-service
  rollback.
- Final host state after wrapper exit: `komari-agent.service` was
  `active/enabled`; `kelicloud-agent-rs.service` was `inactive`.
- Remaining rollout gap: run the same wrapper without `--skip-control` while
  providing `KELICLOUD_PANEL_COOKIE` or `KELICLOUD_PANEL_COOKIE_JAR`, so the
  live panel can create the script exec task and TCP ping task and the Rust
  journal can prove `smoke: task_result_uploaded` plus
  `smoke: ping_result_uploaded`.
- Automation path: the `Real Host Canary` workflow can now run this same full
  wrapper on a self-hosted Linux runner with `control_plane=true`; it uploads
  `real-host-control-canary.evidence.md`, `real-host-control-canary.log`, and
  `service-status.log` as the `kelicloud-agent-rs-real-host-canary` artifact.

2026-06-14 GitHub control-plane workflow dispatch check:

- Commit: `f3b85780a73e4cba0446f50df49229e535fe3519`
- Workflow: `Real Host Canary`
- Run:
  https://github.com/keli-123456/kelicloud-agent-rs/actions/runs/27496579830
- Runner: temporary ephemeral runner registered on
  `vm57463.desivps.com` / `2.56.116.39` with the `kelicloud-canary` label.
- Result: `failed before canary execution`
- Root cause from job log: `KELICLOUD_CANARY_AUTO_DISCOVERY_KEY` repository
  secret was not set. The same job environment also showed empty
  `KELICLOUD_PANEL_COOKIE`, `KELICLOUD_PANEL_USERNAME`, and
  `KELICLOUD_PANEL_PASSWORD`, so the live panel control-plane canary still
  lacks authentication material.
- Host impact: the workflow failed during configuration validation; it did not
  install or start `kelicloud-agent-rs`.
- Post-run cleanup: the ephemeral runner exited and was removed; final host
  state remained `komari-agent.service active/enabled` and
  `kelicloud-agent-rs.service inactive`.
- Remaining rollout gap: configure `KELICLOUD_CANARY_AUTO_DISCOVERY_KEY` plus
  either `KELICLOUD_PANEL_COOKIE` or
  `KELICLOUD_PANEL_USERNAME`/`KELICLOUD_PANEL_PASSWORD` as GitHub repository
  secrets, then rerun `Real Host Canary` with `control_plane=true`. If the old
  Go agent service is present on the self-hosted runner, the auto-discovery key
  secret can be omitted by leaving
  `derive_auto_discovery_from_old_service=true`; panel authentication is still
  required.

2026-06-14 GitHub control-plane workflow derived-key check:

- Commit: `d844a9ac3c055a6e816e704f7e906c8cae23df8e`
- Workflow: `Real Host Canary`
- Run:
  https://github.com/keli-123456/kelicloud-agent-rs/actions/runs/27497188515
- Runner: temporary ephemeral runner registered on
  `vm57463.desivps.com` / `2.56.116.39` with the `kelicloud-canary` label.
- Result: `failed before canary execution`
- Evidence from job log: `CANARY_AUTO_DISCOVERY_KEY` became masked after the
  `Derive auto-discovery from old service` step, proving the workflow parsed
  the key from `komari-agent.service` and no longer requires
  `KELICLOUD_CANARY_AUTO_DISCOVERY_KEY` when the old service is present.
- Remaining failure: `control_plane=true requires KELICLOUD_PANEL_COOKIE or
  KELICLOUD_PANEL_USERNAME/KELICLOUD_PANEL_PASSWORD secrets.`
- Host impact: the workflow failed during configuration validation; it did not
  install or start `kelicloud-agent-rs`.
- Post-run cleanup: the ephemeral runner exited and was removed; final host
  state remained `komari-agent.service active/enabled` and
  `kelicloud-agent-rs.service inactive`.
- Remaining rollout gap: configure either `KELICLOUD_PANEL_COOKIE` or
  `KELICLOUD_PANEL_USERNAME`/`KELICLOUD_PANEL_PASSWORD` as GitHub repository
  secrets, then rerun `Real Host Canary` with `control_plane=true` and
  `derive_auto_discovery_from_old_service=true`.

2026-06-14 live-panel auth audit:

- Local environment audit: no `KELICLOUD_PANEL_*` variables were set in the
  current workstation session.
- Workspace file audit: no obvious cookie jar or panel-auth env file was found
  outside project source/config files.
- DNS audit: `tanzhen2.huhu.icu` resolved through Cloudflare (`172.67.157.9`,
  `104.21.40.207`) rather than directly to the canary host
  `2.56.116.39`.
- Canary host audit: `komari-agent.service` remained `active/enabled`,
  `kelicloud-agent-rs.service` remained `inactive`, and the temporary
  `actions-runner-kelicloud-canary` directory was absent after cleanup.
- GitHub Actions audit for run
  https://github.com/keli-123456/kelicloud-agent-rs/actions/runs/27497188515:
  `Derive auto-discovery from old service` succeeded and
  `Validate canary configuration` failed only because
  `KELICLOUD_PANEL_COOKIE`, `KELICLOUD_PANEL_USERNAME`, and
  `KELICLOUD_PANEL_PASSWORD` were empty. The Rust install/control steps were
  skipped, so the old production agent was not disturbed.
- Current blocker: the full real-host control canary cannot be executed until
  an authenticated live-panel admin cookie or admin username/password is
  provided locally or as GitHub repository secrets.

2026-06-14 full real-host control-plane pass:

- Commit: `73f21e506457501f9b9d3e6755c2c88f7a802832`
- Fix committed before this run: `scripts/real-host-control-canary.sh` now
  restarts `kelicloud-agent-rs.service` before the live control-plane phase and
  waits for both `smoke: report_websocket_connected` and `smoke: report_sent`
  before calling `/api/admin/task/exec`. Earlier attempts reached
  `report_websocket_connected` but failed with `No clients connected` because
  the backend only adds the client to `ws.GetConnectedClients()` after reading
  the first report message.
- Test command:
  `cargo test --test real_host_control_canary_script`
- Host/provider/region: `vm57463.desivps.com` / `2.56.116.39`
- Distro/kernel/arch: `Debian GNU/Linux 12 (bookworm)` /
  `6.1.0-31-amd64` / `x86_64`
- Panel endpoint: `https://tanzhen2.huhu.icu`
- Rust release/version requested: `v0.1.0`
- Evidence directory on host:
  `/root/kelicloud-agent-rs-canary-20260614T115815Z-control-live`
- Evidence file on host:
  `/root/kelicloud-agent-rs-canary-20260614T115815Z-control-live/real-host-control-canary.evidence.md`
- Canary result: `passed`
- Rust client UUID:
  `e6d57d8b-1285-4c00-a2f7-34e590d360dc`
- Script exec task result: `passed`; live panel task ID
  `JWrypdjR72xfDOdy`, and Rust journal evidence
  `smoke: task_result_uploaded task_id=JWrypdjR72xfDOdy exit_code=0`.
- TCP ping task result: `passed`; live panel ping task ID `1`, and Rust
  journal evidence `smoke: ping_result_uploaded task_id=1 value=1`.
- Report/WebSocket readiness evidence from the Rust journal:
  `smoke: auto_discovery_registered`,
  `smoke: basic_info_uploaded`,
  `smoke: report_websocket_connected`, and
  `smoke: report_sent`.
- Final rollback status: `komari-agent.service` returned to `active/enabled`;
  `kelicloud-agent-rs.service` returned to `inactive`.

Live panel control-plane helper:

Integrated real-host control canary:

```bash
sudo KELICLOUD_PANEL_COOKIE='session_token=...' \
scripts/real-host-control-canary.sh \
  --endpoint https://tanzhen2.huhu.icu \
  --auto-discovery <auto-discovery-key> \
  --ping-target 1.1.1.1:443
```

Run this wrapper on the real Linux host when either an authenticated admin
cookie or admin login credentials are available. It downloads the latest
`install.sh`, `canary-install.sh`, and `live-panel-control-smoke.sh` from
`main`, stops the old `komari-agent.service`, installs `kelicloud-agent-rs`,
verifies restart plus install-version pinning, keeps Rust active, parses the
latest `smoke: auto_discovery_registered uuid=...` line from `journalctl`,
triggers one exec task and one TCP ping task through the live panel APIs, then
uninstalls Rust and restores the old service before exit. The generated
`real-host-control-canary.evidence.md` is the preferred evidence file for
closing the remaining real-host exec/ping rollout gap.

Equivalent login-based wrapper invocation:

```bash
sudo KELICLOUD_PANEL_USERNAME='<admin-username>' \
  KELICLOUD_PANEL_PASSWORD='<admin-password>' \
scripts/real-host-control-canary.sh \
  --endpoint https://tanzhen2.huhu.icu \
  --auto-discovery <auto-discovery-key> \
  --ping-target 1.1.1.1:443
```

```bash
KELICLOUD_PANEL_COOKIE='session_token=...' \
scripts/live-panel-control-smoke.sh \
  --endpoint https://tanzhen2.huhu.icu \
  --client <rust-client-uuid> \
  --ping-target 1.1.1.1:443
```

The same helper can log in directly when cookie extraction is inconvenient:

```bash
KELICLOUD_PANEL_USERNAME='<admin-username>' \
KELICLOUD_PANEL_PASSWORD='<admin-password>' \
scripts/live-panel-control-smoke.sh \
  --endpoint https://tanzhen2.huhu.icu \
  --client <rust-client-uuid> \
  --ping-target 1.1.1.1:443
```

Run this helper on the real Linux host during a Rust canary hold window. It
creates one script exec task through `/api/admin/task/exec`, creates one TCP
ping task through `/api/admin/ping/add`, waits for the exec API result, and
waits for `smoke: task_result_uploaded` plus `smoke: ping_result_uploaded` in
`journalctl -u kelicloud-agent-rs`.

Real-host canary evidence template:

- Date:
- Host/provider/region:
- Distro/kernel/arch:
- Panel endpoint:
- Install command source: Rust Linux command generated by kelicloud Web, or
  `scripts/canary-install.sh`.
- Rust release/version requested:
- `scripts/canary-install.sh` result:
- Systemd service status after install:
- Panel online and metrics:
- Script exec task result:
- TCP ping task result:
- Admin WebSSH terminal result:
- `systemctl restart kelicloud-agent-rs` reconnect result:
- Explicit install-version pin/upgrade result:
- Rust uninstall result:
- Go-agent rollback command result:
- Go-agent rollback service name/status:
- GitHub Actions artifact, if used: `kelicloud-agent-rs-real-host-canary`
- Evidence file, if used: `real-host-canary.evidence.md`
- KTP live canary evidence, if KTP TCP is enabled: `ktp-live-canary.evidence.md`
- KTP live canary result:
- Remaining gaps or production rollout notes:

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
