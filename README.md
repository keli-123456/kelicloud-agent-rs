# kelicloud-agent-rs

Linux-only Rust prototype for a future kelicloud agent.

This is not a replacement for the current Go agent yet. The Rust agent is intentionally scoped to Linux servers only:

- Parse endpoint/token flags and environment variables.
- Register with `--auto-discovery` / `AGENT_AUTO_DISCOVERY_KEY` on startup and cache the returned token in `auto-discovery.json`.
- Upload a minimal `/api/clients/uploadBasicInfo` payload.
- Connect `/api/clients/report` over WebSocket.
- Send backend-compatible reports with real CPU, Go-compatible CPU usage floor, memory, swap, disk, uptime, and Go-compatible process metrics.
- Read Linux `/proc` data for load average, uptime, network totals/rate, and TCP/UDP connection counts.
- Calculate RAM and swap from `/proc/meminfo` with Go-agent-compatible byte units.
- Filter physical disks with Go-agent-compatible exclusions for tmpfs, overlay, container mounts, loop devices, and ZFS pool duplicates.
- Honor Go-agent metric flags for NIC include/exclude filters, custom mountpoints, memory include-cache mode, custom IP values, filtered NIC IP fallback, GPU enablement parsing, and `HOST_PROC` process counting.
- Fill BasicInfo with CPU fallback, Go-style arch/OS naming, optional GPU name, custom/NIC/public IP probing, and Linux virtualization/container detection.
- Model optional Go-agent report fields for `gpu` and `cn_connectivity`, including detailed NVIDIA/AMD GPU metrics when available.
- Maintain Go-agent-compatible `net_static.json` samples for `--month-rotate` traffic windows.
- Parse backend control messages for CN connectivity config, terminal, exec, and ping.
- Keep a report loop running with interval sleep, heartbeat ping, and reconnect after send failures.
- Execute backend ping tasks on Linux for TCP, HTTP, and ICMP, then upload `ping_result` messages.
- Execute backend remote exec tasks in the background and upload task results.
- Handle backend WebSSH terminal sessions through a dedicated WebSocket and Linux PTY.
- Avoid printing full tokens in startup output.
- Provide `--once` for a single debug cycle.

## Current CLI

```bash
cargo run -- --endpoint https://panel.example.com --token TOKEN
```

You can also omit `--token` when using backend auto-discovery:

```bash
cargo run -- --endpoint https://panel.example.com --auto-discovery DISCOVERY_KEY
```

Without `--once`, the command keeps running: upload basic info, open the report WebSocket, send reports at `--interval`, send WebSocket heartbeat pings, and reconnect after send failures. With `--once`, it performs one startup cycle and exits. Non-Linux platforms exit with a clear unsupported-platform message.

Supported flags:

- `--endpoint <url>` or `AGENT_ENDPOINT`
- `--token <token>` or `AGENT_TOKEN`
- `--auto-discovery <key>` or `AGENT_AUTO_DISCOVERY_KEY`
- `--insecure`, `--ignore-unsafe-cert`, `AGENT_INSECURE=true`, or `AGENT_IGNORE_UNSAFE_CERT=true`
- `--disable-web-ssh` or `AGENT_DISABLE_WEB_SSH=true`
- `--once` or `AGENT_ONCE=true`
- `--interval <seconds>` or `AGENT_INTERVAL`
- `--max-retries <count>` or `AGENT_MAX_RETRIES`
- `--reconnect-interval <seconds>` or `AGENT_RECONNECT_INTERVAL`
- `--info-report-interval <minutes>` or `AGENT_INFO_REPORT_INTERVAL`
- `--cf-access-client-id <id>` or `AGENT_CF_ACCESS_CLIENT_ID`
- `--cf-access-client-secret <secret>` or `AGENT_CF_ACCESS_CLIENT_SECRET`
- `--include-nics <csv>` or `AGENT_INCLUDE_NICS`
- `--exclude-nics <csv>` or `AGENT_EXCLUDE_NICS`
- `--include-mountpoints <semicolon-list>`, `--include-mountpoint <semicolon-list>`, or `AGENT_INCLUDE_MOUNTPOINTS`
- `--custom-ipv4 <ip>` or `AGENT_CUSTOM_IPV4`
- `--custom-ipv6 <ip>` or `AGENT_CUSTOM_IPV6`
- `--get-ip-addr-from-nic` or `AGENT_GET_IP_ADDR_FROM_NIC=true`
- `--memory-include-cache` or `AGENT_MEMORY_INCLUDE_CACHE=true`
- `--memory-exclude-bcf` or `AGENT_MEMORY_REPORT_RAW_USED=true`
- `--enable-gpu`, `--gpu`, or `AGENT_ENABLE_GPU=true`
- `--month-rotate <day>` or `AGENT_MONTH_ROTATE`
- `--host-proc <path>` or `HOST_PROC`
- `AGENT_TUNNEL_DATA_ENABLED=true` enables the tunnel data plane used by
  tunnel forwarding rules. The installer exposes this as `--enable-tunnel-data`.

## Linux Install

The installer is Linux-only and currently targets systemd. The panel-compatible
auto-discovery command is:

```bash
wget -qO- https://raw.githubusercontent.com/keli-123456/kelicloud-agent-rs/refs/heads/main/install.sh |
  sudo bash -s -- -e https://panel.example.com --auto-discovery DISCOVERY_KEY --enable-tunnel-data
```

You can also install with an explicit client token:

```bash
sudo ./install.sh install \
  --endpoint https://panel.example.com \
  --token TOKEN \
  --enable-tunnel-data
```

The same options can use the shorter panel aliases:

```bash
sudo ./install.sh -e https://panel.example.com -t TOKEN --enable-tunnel-data
```

By default it installs:

- Binary: `/usr/local/bin/kelicloud-agent-rs`
- Environment file: `/etc/kelicloud-agent-rs/config.env`
- Service unit: `/etc/systemd/system/kelicloud-agent-rs.service`

To install a locally built binary instead of downloading a release asset:

```bash
cargo build --release
sudo ./install.sh install \
  --source-binary target/release/kelicloud-agent-rs \
  --endpoint https://panel.example.com \
  --token TOKEN
```

To upgrade or pin a specific GitHub release from the panel-style path:

```bash
wget -qO- https://raw.githubusercontent.com/keli-123456/kelicloud-agent-rs/refs/heads/main/install.sh |
  sudo bash -s -- -e https://panel.example.com --auto-discovery DISCOVERY_KEY --enable-tunnel-data --install-version v0.1.0
```

To roll back to the Go agent, uninstall the Rust service first:

```bash
sudo ./install.sh uninstall
```

Then run the Go agent install command generated by the panel for the same
server or group.

Service operations:

```bash
sudo ./install.sh restart
./install.sh status
sudo ./install.sh uninstall
sudo ./install.sh uninstall --keep-config
```

Installer render helpers are side-effect free and useful for review or CI:

```bash
./install.sh render-service
./install.sh render-env --endpoint https://panel.example.com --token TOKEN
```

## Real Host Install Canary

Use the real-host canary on one expendable Linux systemd server before treating
the Rust agent as the default production Linux agent. This verifies the released
installer path, systemd service, config rendering, restart behavior, optional
release pin or upgrade, uninstall, and optional rollback to the Go agent.

```bash
git clone https://github.com/keli-123456/kelicloud-agent-rs.git
cd kelicloud-agent-rs
sudo bash scripts/canary-install.sh \
  --endpoint https://panel.example.com \
  --auto-discovery DISCOVERY_KEY \
  --install-version v0.1.0 \
  --keep-installed
```

During the observation window, open the kelicloud panel for that server and
trigger one script exec task, one TCP ping task, and one WebSSH terminal. Record
the results in `docs/smoke-compatibility.md`.

For rollback verification, omit `--keep-installed` and pass the Go-agent command
generated by the panel:

```bash
sudo bash scripts/canary-install.sh \
  --endpoint https://panel.example.com \
  --auto-discovery DISCOVERY_KEY \
  --install-version v0.1.0 \
  --rollback-command '<panel generated Go agent install command>'
```

After running the rollback command, the canary waits for
`kelicloud-agent.service` to become active. If the panel command uses a custom
Go-agent service name, pass `--rollback-service-name <name>`.

You can also run the same real-host canary from GitHub Actions on a self-hosted
Linux runner. Label the runner with `kelicloud-canary`, add
`KELICLOUD_CANARY_AUTO_DISCOVERY_KEY`, optionally add
`KELICLOUD_CANARY_ENDPOINT` and `KELICLOUD_CANARY_ROLLBACK_COMMAND`, then run
the `Real Host Canary` workflow manually. The workflow uploads
`kelicloud-agent-rs-real-host-canary` logs for the release evidence record,
including `real-host-canary.evidence.md`.

## Live Smoke Test

For a cross-platform data-plane check against a real backend, use the backend
protocol smoke helper. It does not collect host metrics or run Linux-only
control-plane handlers; it sends deterministic Linux-like basic info and report
payloads through the same HTTP and WebSocket transports used by the agent:

```bash
AGENT_ENDPOINT=https://panel.example.com \
AGENT_TOKEN=TOKEN \
cargo run --locked --bin backend-protocol-smoke
```

Use the Linux-only smoke script when you want to test against a real kelicloud
backend without committing secrets:

```bash
AGENT_ENDPOINT=https://panel.example.com \
AGENT_TOKEN=TOKEN \
scripts/smoke-live.sh
```

Or use auto-discovery instead of a static token:

```bash
AGENT_ENDPOINT=https://panel.example.com \
AGENT_AUTO_DISCOVERY_KEY=KEY \
scripts/smoke-live.sh
```

The default `once` mode builds the release binary, uploads basic info, connects
the report WebSocket, sends one report, and requires `agent loop: completed` in
the captured log. Token values are redacted in script output.

For control-plane checks, keep the agent alive and trigger actions from the
kelicloud panel while the script runs:

```bash
scripts/smoke-live.sh \
  --mode live \
  --duration 120 \
  --endpoint https://panel.example.com \
  --token TOKEN
```

During live mode, verify that ping tasks, script exec tasks, and terminal
sessions reach the agent. The script treats the configured duration being
reached as success because the normal agent loop is expected to keep running.
Use `docs/smoke-compatibility.md` as the first-run checklist and compatibility
watchlist.

After a successful run, the smoke script prints a Markdown compatibility
summary and writes a sibling `*.summary.md` file next to the captured log. The
summary is based on non-secret `smoke:` milestone lines from the agent, such as
basic-info upload, report WebSocket connection, report send, ping result upload,
task result upload, terminal session start, and CN connectivity config receipt.
You can also summarize an existing log directly:

```bash
cargo run --locked --bin smoke-summary -- /tmp/kelicloud-agent-rs-smoke.example.log
```

When you intentionally trigger all control-plane actions during `live` mode, add
`--require-summary-pass` so the smoke run fails if the summary is missing ping,
exec, terminal, or CN connectivity evidence:

```bash
scripts/smoke-live.sh \
  --mode live \
  --duration 120 \
  --require-summary-pass \
  --endpoint https://panel.example.com \
  --token TOKEN
```

You can also run the same smoke test from GitHub Actions when you do not have a
Linux shell on your workstation:

1. Add repository secret `KELICLOUD_SMOKE_TOKEN` or
   `KELICLOUD_SMOKE_AUTO_DISCOVERY_KEY`.
2. Optionally add repository secret `KELICLOUD_SMOKE_ENDPOINT`.
3. If the panel is behind Cloudflare Access, also add
   `KELICLOUD_SMOKE_CF_ACCESS_CLIENT_ID` and
   `KELICLOUD_SMOKE_CF_ACCESS_CLIENT_SECRET`.
4. Open the `Smoke` workflow, choose `Run workflow`, and select `once` or `live`.
5. If `KELICLOUD_SMOKE_ENDPOINT` is not set, fill the `endpoint` workflow input.
6. Fill optional `custom_dns` or set `insecure` when the target environment needs them.
   Enable `require_summary_pass` only when you will trigger all listed control-plane actions.
7. Download the `kelicloud-agent-rs-smoke-logs` artifact after the run.
   It includes both raw `*.log` files and generated `*.summary.md` files.

For a repeatable real-backend control-plane smoke, use the local backend smoke
entry point on Linux:

```bash
scripts/smoke-local-backend.sh
```

That script clones `keli-123456/kelicloud`, prepares the web bundle with the
backend's `scripts/prepare-frontend.sh`, starts a MySQL-backed kelicloud server,
creates a client token, runs `kelicloud-agent-rs`, and then drives admin APIs
for CN connectivity config, script exec, TCP ping, and WebSSH terminal. It ends
by running `smoke-summary --require-pass` against the captured agent log. The
same path runs automatically in the `Local Backend Smoke` GitHub Actions
workflow on pushes to `main`.

To run that same real-backend smoke through the KTP TCP data carrier instead
of the WebSocket data carrier, enable the opt-in KTP mode:

```bash
KELICLOUD_SMOKE_KTP_TCP=true scripts/smoke-local-backend.sh
```

In KTP mode the script starts the backend KTP TCP relay, passes the relay
address to `kelicloud-agent-rs`, verifies the same tunnel echo path, waits for
`tunnel data diagnostics`, and writes
`smoke-logs/ktp-live-canary.evidence.md`.

## Release Builds

GitHub Actions publishes Linux binaries when a version tag is pushed:

```bash
git tag v0.1.0
git push origin v0.1.0
```

The release workflow builds static musl binaries with `cross` and uploads assets
that match the installer download names:

- `kelicloud-agent-rs-linux-amd64`
- `kelicloud-agent-rs-linux-arm64`
- `kelicloud-agent-rs-linux-armv7`

## Verification

```bash
cargo fmt --all -- --check
cargo check --locked --all-targets
cargo test --locked --all-targets
```

GitHub Actions runs the same checks on Linux for pushes to `main` and pull requests.

## Next Milestones

1. Run the live smoke test against a real kelicloud backend and record the first compatibility gaps.
2. Add signed checksum files for release assets.
3. Expand installer support after systemd deployment is stable.
