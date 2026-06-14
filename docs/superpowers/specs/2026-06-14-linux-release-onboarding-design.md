# Linux Release Onboarding Design

## Goal

Make `kelicloud-agent-rs` the default onboarding and upgrade path for Linux
servers while keeping the current Go agent path for Windows and macOS.

This closes the minimum release loop for the Rust agent: release assets,
installer compatibility, backend/web install command generation, group upgrade,
and documented canary evidence.

## Current Context

`kelicloud-agent-rs` already has:

- Linux-only runtime behavior.
- A systemd-focused `install.sh`.
- A GitHub Release workflow that builds these Linux assets:
  `kelicloud-agent-rs-linux-amd64`, `kelicloud-agent-rs-linux-arm64`, and
  `kelicloud-agent-rs-linux-armv7`.
- Passing real backend `Local Backend Smoke` for auto-discovery registration,
  forced token rotation, report reconnect, CN connectivity config, exec, TCP
  ping, and admin WebSSH terminal.

The existing kelicloud backend and web UI still assume the Go agent install
contract:

- Default script source points to `keli-123456/kelicloud-agent`.
- Generated install commands call `install.sh -e <endpoint> --auto-discovery
  <key>`.
- Group upgrade checks `keli-123456/kelicloud-agent` releases and expects Go
  agent asset names such as `kelicloud-agent-linux-amd64`.
- Cloud auto-connect and failover auto-connect reuse the same install script
  source and old CLI-style install snippet.

The Rust installer currently uses a subcommand style:
`install.sh install --endpoint <url> --token <token>`. It does not yet accept
the old panel-generated aliases (`-e`, `-t`, `--install-version`,
`--install-ghproxy`) or `--auto-discovery`, so the panel cannot safely switch
Linux commands to Rust without installer compatibility work.

## Product Decision

Linux should default to the Rust agent. Windows and macOS should keep the Go
agent path.

Reasons:

- The Rust agent intentionally targets Linux only.
- The Linux control-plane smoke is already green against a real backend.
- Keeping Windows/macOS on Go avoids breaking existing non-Linux installs.
- A default Linux switch creates a real canary path without forcing operators to
  understand two agents.

The UI should still make the choice visible. Linux command dialogs should label
the generated command as `Rust Agent (Linux)` and mention that Windows/macOS
continue to use the legacy agent.

## Installer Contract

`kelicloud-agent-rs/install.sh` should support both contracts:

1. Native Rust subcommand style:

   ```bash
   sudo bash install.sh install --endpoint https://panel.example.com --token TOKEN
   ```

2. Panel-compatible Go-agent style:

   ```bash
   wget -qO- https://raw.githubusercontent.com/keli-123456/kelicloud-agent-rs/refs/heads/main/install.sh |
     sudo bash -s -- -e https://panel.example.com --auto-discovery KEY
   ```

Aliases required for panel compatibility:

- `-e`, `--endpoint`: maps to `AGENT_ENDPOINT`.
- `-t`, `--token`: maps to `AGENT_TOKEN`.
- `--auto-discovery`: maps to `AGENT_AUTO_DISCOVERY_KEY`.
- `--install-version`: maps to release `--version`.
- `--install-ghproxy`: maps to `--github-proxy`.
- `--install-dir`: maps to `--bin` by installing
  `<dir>/kelicloud-agent-rs` and stores config under `<dir>/config.env` only
  when explicitly requested by the command generator.
- `--disable-web-ssh`, `--ignore-unsafe-cert`, `--memory-include-cache`,
  `--include-nics`, `--exclude-nics`, `--include-mountpoint`,
  `--month-rotate`: should emit the equivalent `AGENT_*` environment values
  already supported by the Rust runtime.

Install should be idempotent:

- Existing service is replaced or restarted.
- Existing config is overwritten when endpoint/token/auto-discovery values are
  supplied.
- `uninstall --keep-config` keeps config and cache files.
- `uninstall` removes the systemd unit, binary, config file, and auto-discovery
  cache file managed by this installer.

The installer remains Linux/systemd-only for this milestone.

## Backend Contract

The backend should expose explicit install script source handling for agent
flavors:

- `go`: existing default source,
  `https://raw.githubusercontent.com/keli-123456/kelicloud-agent/refs/heads/main`.
- `rust`: Rust source,
  `https://raw.githubusercontent.com/keli-123456/kelicloud-agent-rs/refs/heads/main`.

Where the backend generates Linux auto-connect snippets for cloud providers,
failover, and failover v2, it should use the Rust script source by default.
The generated snippet should still call the panel-compatible flags
`-e <endpoint> --auto-discovery <scoped-key>` so the script remains readable and
close to the browser-generated command.

The existing `base_scripts_url` setting should keep its meaning for the
legacy/Go agent path. A Rust-specific default avoids forcing operators to edit
settings just to use the new Linux default. If a later setting is needed for
custom Rust forks, it can be added after the first release loop.

## Web Contract

The web UI should choose agent flavor from platform:

- Linux install command: Rust agent script source and Rust-compatible options.
- Windows/macOS install command: existing Go agent script source and behavior.
- Group install command: Linux group commands clear the Rust
  `auto-discovery.json` cache path before re-enrolling.
- Group upgrade:
  - Linux nodes query `keli-123456/kelicloud-agent-rs` latest release.
  - Linux nodes require Rust asset names:
    `kelicloud-agent-rs-linux-amd64`,
    `kelicloud-agent-rs-linux-arm64`, or
    `kelicloud-agent-rs-linux-armv7`.
  - Windows/macOS nodes continue querying `keli-123456/kelicloud-agent`.

The command dialog should avoid showing unsupported Rust options for non-Linux
platforms. Linux can keep advanced options that the Rust runtime supports. Any
Go-agent-only option should either be translated to a Rust env value or hidden
when Linux/Rust is selected.

## Canary Evidence

The minimum canary should prove one Linux host can:

1. Install from the generated Linux Rust command with auto-discovery.
2. Appear online in the panel.
3. Receive and execute a script task.
4. Run a TCP ping task.
5. Open an admin WebSSH terminal.
6. Restart the systemd service and reconnect.
7. Run the generated Linux upgrade command and reconnect.
8. Uninstall or roll back to the Go agent path if needed.

The GitHub `Local Backend Smoke` remains the automated proof for backend
protocol compatibility. Manual canary notes should live in
`docs/smoke-compatibility.md` until there is a dedicated release checklist.

## Testing

Agent repo:

- Installer tests cover old-style aliases, auto-discovery env rendering,
  install-version mapping, github proxy mapping, and uninstall cache removal.
- Release workflow tests continue to assert the Rust Linux asset names.
- Run:
  - `cargo fmt --all -- --check`
  - `git diff --check`
  - `cargo test --locked --all-targets`

Backend repo:

- Unit tests cover Rust/default script source generation.
- Auto-connect tests cover Linux snippets using the Rust source.
- Run targeted Go tests for install script source, cloud auto-connect, failover,
  and failover v2 auto-connect code.

Web repo:

- Tests or static assertions cover Linux command generation with Rust source and
  Linux group upgrade asset lookup for `kelicloud-agent-rs`.
- Run the existing type/build checks for the web repo.

Release evidence:

- Push `kelicloud-agent-rs` and confirm CI plus `Local Backend Smoke` pass.
- After backend/web changes, push web first if needed, then push backend so the
  backend workflow prepares the latest web bundle.

## Risks And Limits

- Rust remains Linux/systemd-only. This is an explicit product limit, not a bug
  for this milestone.
- Go agent auto-update parity is not part of this milestone. The Rust installer
  supports explicit upgrade by rerunning the install script.
- Non-systemd Linux hosts remain outside the first release loop.
- A mixed group containing Linux and non-Linux nodes may need split upgrade
  tasks internally because Linux checks Rust releases while non-Linux checks Go
  releases.
- If no Rust GitHub Release exists yet, the generated install command can be
  correct but first-run download will fail. The release/tag gate must happen
  before this is considered production-ready.
