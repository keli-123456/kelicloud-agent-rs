# Tunnel Forwarding Framework Design

## Goal

Design a group-based encrypted tunnel forwarding capability for
`kelicloud-agent-rs` without changing the existing agent monitoring and control
behavior.

The feature is **tunnel forwarding**, not VPN:

- No TUN device.
- No virtual IP allocation.
- No global route management.
- No first-release UDP support.
- No first-release peer-to-peer direct connection.

The first useful release should support TCP forwarding for cases such as RDP:

```text
ingress group :10088 -> Keli Tunnel -> egress group -> 127.0.0.1:3389
```

Single-machine forwarding is represented by a group that contains one machine:

```text
machine-1 group :10088 -> Keli Tunnel -> machine-1 group -> 127.0.0.1:3389
```

## Product Model

The web UI should expose a dedicated admin page named **Tunnel Forwarding**.
This feature should not be hidden inside the server row context menu, because a
tunnel rule is a cross-node runtime object rather than a single-node action.

Every tunnel rule is group based:

- `ingress_group_id`: all online tunnel-capable machines in the group receive
  the same listener rule and listen on the same address and port.
- `egress_group_id`: each new TCP session chooses one healthy online member
  from this group.
- A single TCP session remains pinned to its chosen egress machine for its
  lifetime.
- A group with one machine is the supported way to model a single-machine
  tunnel.

The create form should show only non-empty groups for ingress and egress
selection. Existing rules must continue to display their referenced groups even
if those groups later become empty, and should show a clear unavailable state
instead of silently hiding the rule.

Suggested create/edit fields:

- Name.
- Enabled.
- Ingress group.
- Listen address, default `0.0.0.0`.
- Listen port.
- Egress group.
- Target host, default-friendly value `127.0.0.1`.
- Target port.
- Max concurrent sessions.
- Source allowlist, default `0.0.0.0/0`.
- Remark.

Suggested list columns:

- Status.
- Name.
- Ingress group.
- Listen port.
- Egress group.
- Target service.
- Current sessions.
- Today's traffic.
- Last error.
- Actions.

## Architecture

The tunnel system has three separate planes:

1. **Control plane**
   - Admin APIs, database records, permission checks, rule CRUD, audit logs, and
     status views.
   - Existing agent report/control channels may notify agents that tunnel rules
     changed, but should not carry tunnel data.

2. **Signaling plane**
   - Agent capability registration.
   - Rule sync.
   - Session pairing between ingress, relay, and egress.
   - Health and error reporting.

3. **Data plane**
   - TCP byte forwarding.
   - Keli Tunnel Protocol session frames.
   - Flow control, idle timeout, and traffic accounting.

The first release should run through the backend as a relay/coordinator. A
future release can split relay traffic into a dedicated `kelicloud-relay`
service without changing the rule model.

## Agent Isolation Boundary

Tunnel forwarding is an independent data plane. It must not affect current
`kelicloud-agent-rs` behavior:

- Basic info upload.
- Report WebSocket.
- Metrics.
- Script execution.
- Ping tasks.
- WebSSH terminal.
- Auto-discovery and token recovery.
- Installer and rollback flows.

The agent should be structured like this:

```text
agent main
  |- monitor/report loop
  |- task/ping/terminal handlers
  `- tunnel runtime
       |- rule sync
       |- ingress listeners
       |- relay connection
       `- session workers
```

Tunnel failures must be scoped to the affected rule or session. A relay outage,
listen-port conflict, egress selection failure, or target connection failure
must not make the node offline, stop metrics, break WebSSH, or terminate the
agent process.

During the first rollout, the agent can keep a guarded runtime switch such as
`AGENT_TUNNEL_ENABLED`. The long-term default can be enabled because no tunnel
rules means no listeners and no active tunnel data connections.

## Transport Protocol

The primary transport should be a project-owned binary protocol:

```text
KTP: Keli Tunnel Protocol
```

KTP is a tunnel framing protocol, not a custom cryptographic algorithm. The
encrypted transport should use established libraries such as `rustls`.

First-release transport:

```text
KTP over TLS over TCP
```

WebSocket Secure can remain a compatibility mode later, but should not be the
main high-performance path.

KTP should include these frame classes:

- `HELLO`: protocol version, agent UUID, role, capability list.
- `AUTH`: short-lived relay credential or token proof.
- `RULE_SYNC`: active rule set and revision.
- `SESSION_OPEN`: new inbound TCP session request.
- `SESSION_ACCEPT`: chosen egress confirms it can dial the target.
- `SESSION_DATA`: bidirectional bytes for one session.
- `SESSION_WINDOW`: flow-control credit.
- `SESSION_CLOSE`: normal close with reason.
- `SESSION_ERROR`: typed failure.
- `PING` / `PONG`: keepalive and latency measurement.
- `STATS`: session and rule counters.

Every TCP connection receives a `session_id`. Multiple sessions can share one
agent-to-relay KTP connection.

The first KTP frame must carry:

- Protocol version.
- Agent version.
- Supported frame versions.
- Supported capabilities, such as `tcp`, `multiplex`, `stats`, and later `udp`.

## Backend And Relay Responsibilities

For the first release, the backend can act as the relay:

- Store tunnel rules.
- Validate user ownership for ingress and egress groups.
- Validate that agents in selected groups belong to the same user scope.
- Track tunnel-capable agent sessions.
- Send rule update notifications.
- Accept ingress data-plane connections from agents.
- Choose healthy egress members using round robin.
- Relay KTP frames between ingress and egress agents.
- Record connection stats and recent errors.
- Emit audit logs.

The backend should keep the existing report WebSocket lightweight. Tunnel data
should use a separate endpoint, for example:

```text
/api/clients/tunnel
```

Admin APIs should be separate from the existing client port-forward APIs, for
example:

```text
GET    /api/admin/tunnels
POST   /api/admin/tunnels
PUT    /api/admin/tunnels/:id
DELETE /api/admin/tunnels/:id
POST   /api/admin/tunnels/:id/enable
POST   /api/admin/tunnels/:id/disable
GET    /api/admin/tunnels/:id/status
```

The older iptables/nftables-based port forwarding feature can remain as a
separate legacy capability. It should not be renamed into the new tunnel model
until the new data plane is stable.

## Rule And Status Semantics

A tunnel rule should include:

- User owner UUID.
- Name.
- Enabled flag.
- Ingress group ID.
- Listen address.
- Listen port.
- Egress group ID.
- Target host.
- Target port.
- Source allowlist.
- Max concurrent sessions.
- Created/updated timestamps.
- Last applied revision.
- Last error.

Status values should be explicit:

- `ok`: all expected online ingress members are listening and at least one
  egress member is healthy.
- `partial`: some ingress members or egress members are unavailable, but the
  rule can still serve traffic.
- `disabled`: rule is saved but inactive.
- `empty_ingress_group`: no ingress machines are available.
- `empty_egress_group`: no egress machines are available.
- `unsupported_agent`: group contains machines that do not support tunnels.
- `listen_failed`: at least one ingress machine could not bind the listen port.
- `relay_unavailable`: agent cannot reach relay.
- `target_failed`: selected egress could not connect to target service.
- `auth_failed`: tunnel credential or ownership check failed.

Port conflict rules:

- Within the same ingress group, enabled TCP rules must not reuse the same
  listen address and listen port.
- If one machine has a local port conflict, only that machine should report
  `listen_failed`; other machines in the ingress group can continue serving the
  rule.

## Security

Security requirements:

- Admins can only see and select groups within their own user scope.
- Tunnel rules cannot cross user scopes.
- A relay credential must be scoped to one agent, rule revision, role, and
  expiration.
- Agents must not accept tunnel rules that are not signed or authorized by the
  backend.
- Source allowlists should be supported from the first UI/API release.
- Target hosts should allow loopback, private IPs, and ordinary DNS names, with
  validation and clear UI warnings.
- Audit logs should record create, update, delete, enable, disable, rule sync,
  session failures, and high-level session start/stop metadata.

The project must not implement custom encryption. KTP frames ride over a
mature encrypted transport.

## Resource Limits

Default limits should exist before the feature is enabled broadly:

- Max active rules per user.
- Max active rules per agent.
- Max sessions per rule.
- Max sessions per agent.
- Max per-session buffer.
- Handshake timeout.
- Dial timeout.
- Idle timeout.
- Keepalive interval.
- Reconnect backoff with jitter.

When limits are reached, the affected session should fail with a typed error
and the agent should keep running.

## Data Flow

Rule sync:

1. Admin creates or updates a tunnel rule.
2. Backend validates group ownership, non-empty create-time selections, port
   conflicts, and target fields.
3. Backend increments the rule revision.
4. Backend notifies online tunnel-capable agents in the ingress and egress
   groups that rule sync is needed.
5. Agents fetch or receive their role-specific rule set.
6. Ingress agents bind listeners for enabled rules.
7. Egress agents mark themselves available for matching rule revisions.

Session flow:

1. A user connects to an ingress machine listen port.
2. Ingress agent opens a KTP session to relay with `SESSION_OPEN`.
3. Relay selects one healthy egress agent from the egress group using round
   robin.
4. Egress agent dials the configured target host and target port.
5. Egress sends `SESSION_ACCEPT` or `SESSION_ERROR`.
6. Ingress and egress exchange `SESSION_DATA` through relay.
7. Either side sends `SESSION_CLOSE`, or the relay closes the session on
   timeout or error.
8. Backend updates counters and recent error/status fields.

## Error Handling

Errors should be typed and visible:

- Rule validation errors are returned directly from admin APIs.
- Agent rule-sync failures update per-agent rule status.
- Listen failures do not stop the agent.
- Relay connection failures use bounded reconnect backoff.
- Egress target dial failures close only the affected session.
- Protocol version mismatches mark the agent as unsupported for tunnel rules.
- Authentication failures stop the tunnel connection and are audit logged.

The UI should show both rule-level status and per-member detail so operators can
distinguish "the whole tunnel is down" from "one ingress machine failed to
listen".

## Rollout Plan

The feature should be delivered in stages:

1. **Framework and schema**
   - Rule models, admin APIs, group filtering, status model, and UI shell.
   - No data traffic yet.

2. **KTP handshake and capability registration**
   - Separate `/api/clients/tunnel` connection.
   - Protocol version and capability exchange.
   - No listener yet.

3. **TCP ingress and relay MVP**
   - Ingress listener.
   - Relay session creation.
   - Egress target dial.
   - Bidirectional TCP forwarding.
   - RDP smoke on `127.0.0.1:3389`.

4. **Operations hardening**
   - Resource limits.
   - Source allowlists.
   - Stats and recent errors.
   - Per-member status.
   - Audit logs.

5. **Performance work**
   - Flow control tuning.
   - Buffer tuning.
   - Optional compression only for protocols where it helps.
   - Dedicated relay service if backend relay load becomes too high.

Later stages can add raw TLS performance improvements, WSS compatibility mode,
UDP framing, and direct relay placement options.

## Testing

Agent tests:

- KTP frame encode/decode and version negotiation.
- Rule sync applies only authorized role-specific rules.
- Ingress bind conflict reports rule failure without stopping report loop.
- Session data forwarding with fake relay and fake target.
- Resource limit enforcement.
- Tunnel runtime failure does not stop metrics/report tests.

Backend tests:

- User-scope validation for ingress and egress groups.
- Empty groups hidden for create but preserved for existing rules.
- Port conflict detection within ingress groups.
- Round-robin egress selection skips offline and unsupported agents.
- Rule status aggregation.
- Audit log creation for rule and session lifecycle.

Web tests:

- Tunnel page list renders rule statuses.
- Create form hides empty groups.
- Edit form preserves currently selected empty group with an unavailable label.
- Validation prevents duplicate listen ports in the same ingress group.
- Per-member status distinguishes full outage from partial availability.

Integration smoke:

- One-machine RDP-style tunnel:
  `machine-1 group :10088 -> machine-1 group -> 127.0.0.1:3389`.
- Two-group tunnel:
  `ingress group :10088 -> egress group -> 127.0.0.1:3389`.
- Egress group round robin across at least two targets.
- Existing exec, ping, WebSSH, and metrics continue passing while tunnel traffic
  is active.

## Non-Goals For First Release

- VPN behavior.
- TUN/TAP devices.
- Virtual IP management.
- UDP.
- P2P direct connections.
- Browser-based tunnel clients.
- Replacing the legacy iptables/nftables port-forward feature.
- Self-designed cryptography.
