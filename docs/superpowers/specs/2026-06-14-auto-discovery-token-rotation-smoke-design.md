# Auto-Discovery Token Rotation Smoke Design

## Goal

Verify that `kelicloud-agent-rs` can recover from a real backend token rotation
when it is started with `--auto-discovery`, and prove that post-recovery control
plane actions still work.

## Current Context

`scripts/smoke-local-backend.sh` already starts a real kelicloud backend,
prepares the latest web bundle through `scripts/prepare-frontend.sh`, creates a
client, starts the Rust agent, triggers CN connectivity config, exec, TCP ping,
and admin WebSSH terminal, then gates the run with `smoke-summary
--require-pass`.

The Rust agent already has unit coverage for auto-discovery cache loading,
registration, invalid-token recovery during basic-info upload/report WebSocket
connection, and shared-token propagation to task result and terminal
connectors. What is missing is a real backend smoke that forces the token to
change while the agent is alive.

## Proposed Flow

The local backend smoke should switch from a static admin-created client token
to backend auto-discovery:

1. Log in as admin.
2. Read `/api/admin/settings/` and extract `auto_discovery_key`.
3. Start `kelicloud-agent-rs` with `--auto-discovery <key>` and a deterministic
   `HOSTNAME=agent-rs-smoke` so the backend client name becomes
   `Auto-agent-rs-smoke`.
4. Poll `/api/admin/client/list` until that client appears, then capture its
   UUID and current token through `/api/admin/client/:uuid/token`.
5. Use the real admin edit API, `/api/admin/client/:uuid/edit`, to replace the
   backend token with a new unique value. This invalidates the agent's cached
   token without directly editing the database.
6. Wait for the agent to hit an invalid-token path, clear its
   `auto-discovery.json`, re-register with the backend, and resume reporting.
7. Re-resolve the active auto-discovered client UUID from the admin list and run
   CN connectivity, exec, TCP ping, and admin terminal against the recovered
   client.
8. Run `smoke-summary --require-pass` as the final gate.

Using the admin API is intentionally closer to a real operator action than a SQL
update. Deleting the client is not used because it changes the UUID and mixes
token recovery with deletion/offline cleanup behavior.

## Evidence

The smoke should leave non-secret evidence in `agent.log` and helper logs:

- Initial auto-discovery registration succeeded.
- A stale token produced an invalid-token recovery path.
- A second auto-discovery registration succeeded.
- The recovered agent sent reports.
- Exec, TCP ping, CN connectivity config, and admin terminal all work after the
  recovery.
- `live smoke duration reached` is still present so the summary can treat the
  long-running agent loop as healthy.

The script may log old/new token prefixes only through existing redaction
helpers or avoid token content entirely.

## Testing

Add script-level tests first in `tests/local_backend_smoke_script.rs` that check
for the new smoke stages and safety properties:

- The script reads `/api/admin/settings/`.
- The script starts the agent with `--auto-discovery` instead of `--token`.
- The script sets deterministic `HOSTNAME`.
- The script polls `/api/admin/client/list`.
- The script edits `/api/admin/client/:uuid/edit` to rotate the token.
- The script waits for auto-discovery recovery evidence before triggering exec,
  ping, and terminal.

After implementation, run:

- `cargo test --locked --test local_backend_smoke_script -- --nocapture`
- `cargo fmt --all -- --check`
- `git diff --check`
- `cargo test --locked --all-targets`
- Push to `main` and require the `Local Backend Smoke` GitHub Actions workflow
  to pass.

## Risks

The backend creates a new client on each auto-discovery registration. The smoke
must re-resolve `CLIENT_UUID` after recovery instead of assuming the original
client UUID remains active.

The report WebSocket may not notice token changes until reconnect or the next
basic-info upload. The smoke should wait for concrete recovery evidence before
triggering control-plane actions, not rely on fixed sleeps.

The Windows workstation cannot run the Linux smoke locally, so GitHub Actions
remains the authoritative full-run evidence.
