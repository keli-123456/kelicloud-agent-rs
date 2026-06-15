# Tunnel Forwarding Phase 2 Control Plane Design

## Goal

Build the first live tunnel control-plane slice across `kelicloud` and
`kelicloud-agent-rs`.

This phase proves that tunnel-capable Linux agents can connect to the backend,
register capabilities, receive group-based tunnel rule revisions, and report
rule health without opening any local listener or forwarding any traffic.

This phase does not implement:

- KTP frame encoding.
- TLS tunnel data sessions.
- WebSocket tunnel data sessions.
- TCP listener sockets on agents.
- Target dialing from agents.
- Relay byte forwarding.

## Product Boundary

Phase 1 created the admin rule model and `/admin/tunnels` page. Phase 2 makes
those rules observable by agents and makes agent capability visible to the
backend status model.

The web page does not need a new layout in this phase. It can reuse existing
rule statuses such as `ok`, `unsupported_agent`, `relay_unavailable`, and
`partial`. If the backend status improves, the existing page will display the
new status strings through the labels already added in Phase 1.

## Transport Choice

Phase 2 uses a separate JSON WebSocket endpoint:

```text
GET /api/clients/tunnel?token=<client-token>
```

This endpoint is a control channel only. It is intentionally not KTP and not a
data channel. JSON keeps the first integration easy to inspect, test, and debug.
The later KTP/TLS data plane can use a different framing layer while keeping the
same rule and capability model.

The existing `/api/clients/report` WebSocket remains responsible for metrics,
ping, exec, terminal notification, and online presence. Tunnel control must not
write to `ws.GetConnectedClients()` or otherwise decide whether a node is
online for the existing server page.

## Backend Responsibilities

Add a client tunnel endpoint under the existing token-authenticated client API:

```go
tokenAuthrized.GET("/tunnel", client.WebSocketTunnelControl)
```

The handler validates:

- Request is a WebSocket upgrade.
- Client token is valid through `TokenAuthMiddleware`.
- Client UUID and user ID are present in request context.
- The owner has the `tunnels` user feature enabled.

If the feature is disabled, the backend returns a clear control error and closes
the tunnel control socket. It must not disconnect the report WebSocket.

Add a tunnel control service that owns:

- Capability state per client.
- Connected/disconnected state for tunnel control only.
- Rule selection for one client from group-based `TunnelRule` records.
- Rule revision fingerprinting.
- Agent heartbeat and rule status updates.

Persist minimal capability state so admin status survives process boundaries
well enough to explain what happened:

```go
type ClientTunnelState struct {
    ID                    uint
    UserID                string
    ClientUUID            string
    Connected             bool
    AgentVersion          string
    ControlProtocol       string
    CapabilitiesJSON      string
    LastRuleRevision      string
    LastAckRevision       string
    LastHeartbeatAt       models.LocalTime
    LastError             string
    CreatedAt             models.LocalTime
    UpdatedAt             models.LocalTime
}
```

`Connected` is best-effort. On process restart every state can be treated as
disconnected until an agent reconnects.

Rule status calculation should become control-aware:

- `disabled`: rule disabled.
- `empty_ingress_group`: ingress group has no machines.
- `empty_egress_group`: egress group has no machines.
- `unsupported_agent`: a referenced non-empty group has no client that has ever
  registered the required tunnel control capability.
- `relay_unavailable`: at least one required group has known tunnel-capable
  clients, but none are currently connected on the tunnel control endpoint.
- `partial`: one group has connected tunnel-capable clients but the rule has a
  recent per-rule error or only part of the referenced groups is ready.
- `ok`: ingress and egress groups each have at least one currently connected
  tunnel-capable client and no rule error.

## Agent Responsibilities

`kelicloud-agent-rs` gets a small, isolated tunnel control runtime:

```text
agent main
  |- existing report/runtime loop
  |- existing task/ping/terminal handlers
  `- tunnel control runtime
       |- connects /api/clients/tunnel
       |- sends hello and heartbeat
       |- receives rule_sync
       `- reports rule ack/status
```

The tunnel control runtime must never stop the normal report loop. Connection
failure, feature denial, invalid rule payloads, and backend 404/403 responses
are logged and retried with backoff, but they do not make the agent exit.

Configuration:

- Add `AGENT_TUNNEL_CONTROL_ENABLED`.
- Default behavior is `auto`: try tunnel control when the config does not turn
  it off, but back off quietly if the backend does not support the endpoint.
- `0`, `false`, or `disabled` disables the runtime.

This preserves compatibility with older panels while allowing new deployments to
start registering capability without another installer flag.

## Message Contract

Agent to backend:

```json
{
  "type": "hello",
  "control_protocol": "keli-tunnel-control.v1",
  "agent_version": "0.1.0",
  "capabilities": ["tunnel_control", "rule_sync", "status_report"],
  "data_plane": false
}
```

```json
{
  "type": "heartbeat",
  "last_rule_revision": "rev",
  "active_rules": []
}
```

```json
{
  "type": "rule_ack",
  "revision": "rev",
  "accepted_rule_ids": [1, 2],
  "rejected_rules": [
    { "id": 3, "error": "unsupported protocol" }
  ]
}
```

```json
{
  "type": "rule_status",
  "revision": "rev",
  "rules": [
    { "id": 1, "status": "ok", "error": "" }
  ]
}
```

Backend to agent:

```json
{
  "type": "hello_ack",
  "server_protocol": "keli-tunnel-control.v1",
  "heartbeat_interval_seconds": 15
}
```

```json
{
  "type": "rule_sync",
  "revision": "rev",
  "rules": [
    {
      "id": 1,
      "name": "RDP",
      "enabled": true,
      "protocol": "tcp",
      "role": "ingress",
      "ingress_group": "edge",
      "listen_address": "0.0.0.0",
      "listen_port": 10088,
      "egress_group": "rdp",
      "target_host": "127.0.0.1",
      "target_port": 3389,
      "source_allowlist": "0.0.0.0/0",
      "max_concurrent_sessions": 32,
      "last_revision": 1
    }
  ]
}
```

```json
{
  "type": "error",
  "code": "feature_disabled",
  "message": "tunnel forwarding is disabled for this account"
}
```

Rules are role-specific. If the client is in both ingress and egress groups, the
backend sends one entry for that rule with `role: "both"`. The allowed role
values are `ingress`, `egress`, and `both`.

## Rule Revision

The backend computes a deterministic string fingerprint from the selected rule
set for one client:

- Rule ID.
- Enabled flag.
- Protocol.
- Ingress and egress groups.
- Listen address and port.
- Target host and port.
- Source allowlist.
- Max concurrent sessions.
- Last revision.
- Role for that client.

Sorting must be stable by `id` and role before hashing. A deleted rule changes
the fingerprint because the selected rule list changes. The first implementation
can use SHA-256 hex.

The agent stores the latest received revision in memory and reports it in
heartbeats. It does not persist rules to disk in this phase.

## Error Handling

Backend:

- Invalid first message: send `error` and close the control socket.
- Feature disabled: send `feature_disabled` and close.
- Client deleted or token invalid: close through existing token auth behavior.
- Rule load failure: send `error` but keep the connection alive when possible.
- Socket close: mark only tunnel control state disconnected.

Agent:

- 401 invalid token: use existing token recovery behavior if available.
- 403 feature disabled: back off longer and do not spam logs.
- 404 endpoint missing: back off quietly for old backend compatibility.
- Invalid `rule_sync`: send `rule_ack` with rejected rules when possible.
- Any tunnel control failure must not stop metrics, exec, ping, or terminal.

## Testing

Backend tests:

- `/api/clients/tunnel` rejects non-WebSocket requests.
- Token-authenticated client can connect when owner has the tunnel feature.
- Feature-disabled owner receives `feature_disabled`.
- Rule selection returns ingress, egress, and both-role rules for a client group.
- Revision fingerprint changes on rule update and deletion.
- Status derivation returns `unsupported_agent`, `relay_unavailable`, and `ok`
  from persisted capability/connection state.

Agent tests:

- Builds `ws://` and `wss://` tunnel control URLs from normal endpoints.
- Parses `hello_ack`, `rule_sync`, and `error` messages.
- Generates `hello`, `heartbeat`, `rule_ack`, and `rule_status` messages.
- Runtime continues without failing the main loop when tunnel control connect
  returns 404, 403, timeout, or invalid JSON.
- No listener sockets are opened by the tunnel control runtime.

Integration smoke:

- A fake backend accepts `/api/clients/tunnel`.
- Agent sends `hello`.
- Backend sends `hello_ack` and `rule_sync`.
- Agent sends `rule_ack` and heartbeat with the same revision.

## Success Criteria

- Backend exposes `/api/clients/tunnel` and records tunnel capability state.
- `kelicloud-agent-rs` can connect to the control endpoint without affecting the
  existing report WebSocket.
- Rule sync payloads are scoped to the authenticated client owner and group.
- Admin tunnel rule status can distinguish no capability, disconnected control
  channel, and ready control channel.
- Unit tests cover protocol parsing, revisioning, user ownership, feature gate,
  and no-fatal agent behavior.
- No Phase 2 code opens TCP listeners, dials target services, relays bytes, or
  implements KTP/TLS data frames.

## Open Follow-Ups For Later Phases

- KTP binary frame format and TLS transport.
- Backend relay session pairing.
- Ingress listener management.
- Egress target dialing.
- Flow control and traffic accounting.
- Web UI live session counters.
