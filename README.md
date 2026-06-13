# kelicloud-agent-rs

Linux-only Rust prototype for a future kelicloud agent.

This is not a replacement for the current Go agent yet. The Rust agent is intentionally scoped to Linux servers only:

- Parse endpoint/token flags and environment variables.
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

Without `--once`, the command keeps running: upload basic info, open the report WebSocket, send reports at `--interval`, send WebSocket heartbeat pings, and reconnect after send failures. With `--once`, it performs one startup cycle and exits. Non-Linux platforms exit with a clear unsupported-platform message.

Supported flags:

- `--endpoint <url>` or `AGENT_ENDPOINT`
- `--token <token>` or `AGENT_TOKEN`
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

## Linux Install

The installer is Linux-only and currently targets systemd:

```bash
sudo ./install.sh install \
  --endpoint https://panel.example.com \
  --token TOKEN
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

## Verification

```bash
cargo fmt --all -- --check
cargo check --locked --all-targets
cargo test --locked --all-targets
```

GitHub Actions runs the same checks on Linux for pushes to `main` and pull requests.

## Next Milestones

1. Add release builds for common Linux architectures.
2. Run an end-to-end smoke test against a real kelicloud backend.
3. Expand installer support after systemd deployment is stable.
