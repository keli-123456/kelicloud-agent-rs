# Tunnel Forwarding Phase 3 Data Plane Design

## Goal

Design the tunnel forwarding data plane protocol and runtime framework for
`kelicloud` and `kelicloud-agent-rs`.

This phase defines how TCP tunnel sessions will be represented, authorized,
paired, multiplexed, and tested. It does not implement real forwarding code,
does not open TCP listeners, and does not add KTP, TLS, or WebSocket data-plane
runtime code.

## Decision Summary

Phase 3 chooses this first data-plane direction:

```text
KTP over WebSocket Binary over HTTPS
```

KTP is the project-owned frame protocol. WebSocket is only the first carrier.
This keeps deployment simple because the first MVP can reuse the existing panel
HTTPS entry point and common reverse proxy setups. A later high-performance
carrier can add:

```text
KTP over raw TLS over TCP
```

The KTP frame model must not depend on WebSocket semantics. One WebSocket binary
message carries one KTP frame in the MVP, but the frame still includes its own
length field so the same frame can later ride over raw TLS.

## Non-Goals

Phase 3 does not design or implement:

- VPN behavior.
- TUN/TAP devices.
- UDP forwarding.
- Peer-to-peer direct agent connections.
- Raw TLS listener implementation.
- Browser tunnel clients.
- Local desktop client connectors.
- Replacement of the legacy iptables/nftables port-forward feature.
- Custom cryptography.
- Real TCP listener, target dial, or byte relay code.

## Existing Foundation

Phase 1 provided the rule model and admin surface:

- `TunnelRule`
- ingress group
- egress group
- listen address and port
- target host and port
- source allowlist
- max concurrent sessions
- status strings

Phase 2 provided the control plane:

- `/api/clients/tunnel` JSON WebSocket.
- agent capability registration.
- role-specific `rule_sync`.
- deterministic rule revision.
- `rule_ack`, heartbeat, and `rule_status`.
- `ClientTunnelState`.

Phase 3 keeps that control channel intact. The data plane uses a separate
endpoint and protocol so metrics, script execution, ping tasks, terminal, and
control rule sync remain isolated.

## Endpoints

Control plane, already present:

```text
GET /api/clients/tunnel?token=<client-token>
```

Data plane, designed in this phase:

```text
GET /api/clients/tunnel/data?token=<client-token>
```

The data endpoint accepts WebSocket upgrades and only processes binary messages.
Text messages, non-WebSocket requests, invalid tokens, disabled accounts, or
users without the `tunnels` feature are rejected.

The data endpoint does not decide whether a node is online on the server page.
It owns only tunnel data-plane readiness.

## Runtime Architecture

The agent process remains split into independent runtimes:

```text
agent main
  |- existing monitor/report loop
  |- existing task/ping/terminal handlers
  |- tunnel control runtime
  `- tunnel data runtime
       |- maintains one data WebSocket to backend
       |- applies current role-specific rules from control runtime
       |- binds ingress listeners for ingress/both rules
       |- dials targets for egress/both session opens
       `- multiplexes TCP sessions over KTP frames
```

The backend acts as relay in the first implementation:

```text
TCP client
  -> ingress agent listener
  -> KTP data connection
  -> backend relay
  -> egress agent KTP data connection
  -> target TCP service
```

The backend does not trust target information supplied by ingress agents. It
loads the target host and port from the database rule that belongs to the
authenticated user.

## Agent Roles

Each selected rule has one role for each agent:

- `ingress`: the agent may bind the rule listener and open sessions to relay.
- `egress`: the agent may accept session opens from relay and dial the target.
- `both`: the agent may do both for the same rule.

### Ingress

An ingress agent:

1. Receives a rule with role `ingress` or `both`.
2. Validates source allowlist locally.
3. Binds `listen_address:listen_port`.
4. Accepts TCP client connections.
5. Creates a KTP `SESSION_OPEN` for each accepted connection.
6. Sends TCP bytes as `SESSION_DATA`.
7. Closes only the affected session on errors.

Listen failures are reported through Phase 2 `rule_status` as `listen_failed`.
They must not stop the report loop or other rules.

### Egress

An egress agent:

1. Receives a rule with role `egress` or `both`.
2. Keeps data-plane readiness for that rule revision.
3. Receives relay-originated `SESSION_OPEN`.
4. Dials the backend-supplied `target_host:target_port`.
5. Sends `SESSION_ACCEPT` or `SESSION_ERROR`.
6. Sends target bytes as `SESSION_DATA`.

Target dial failures close only the affected session and are reported as
`target_failed`.

### Same-Machine Loopback

A group with one machine can model:

```text
machine-1 :10088 -> relay -> machine-1 127.0.0.1:3389
```

The first MVP still routes through the backend relay even when ingress and
egress are the same agent. This keeps authorization, accounting, status, and
failure handling identical to the two-agent path.

The protocol must support same-connection loopback. KTP frames therefore carry a
`leg` field so the same agent data connection can demultiplex ingress-side and
egress-side frames for the same `session_id`.

Future optimization may add local fast-path forwarding for same-machine rules,
but it is not part of the MVP.

## First-Hop Visibility Boundary

If a user connects an ordinary RDP client directly to the public ingress
listener, the client-to-ingress segment is still ordinary RDP over TCP.

The MVP hides and protects the traffic between:

```text
ingress agent -> backend relay -> egress agent
```

Fully hiding the first hop requires a later client-side tunnel connector or a
local KTP listener on the user's machine. That is outside the first MVP.

## KTP Frame Format

KTP v1 uses a fixed binary header followed by a frame-type-specific payload.
All integer fields use network byte order.

```text
0               4               8               12
+---------------+---------------+---------------+
| magic "KTP1"  | ver | type    | leg | flags   |
+---------------+---------------+---------------+
| session_id                                    |
+-----------------------------------------------+
| payload_len                  | reserved       |
+-----------------------------------------------+
| payload bytes ...                             |
+-----------------------------------------------+
```

Header fields:

- `magic`: four bytes, always `KTP1`.
- `version`: one byte, first version is `1`.
- `type`: one byte frame type.
- `leg`: one byte frame leg:
  - `0`: connection-level frame.
  - `1`: ingress leg.
  - `2`: egress leg.
- `flags`: one byte for frame flags in v1.
- `session_id`: unsigned 64-bit session identifier. It is `0` for
  connection-level frames.
- `payload_len`: unsigned 32-bit payload length.
- `reserved`: unsigned 32-bit reserved field, must be `0` in v1.

The fixed header is 24 bytes.

Frame parsers must reject:

- wrong magic.
- unsupported version.
- unknown frame type.
- invalid leg for the frame type.
- non-zero reserved field in strict tests.
- `payload_len` above the configured maximum.
- truncated payloads.

## Frame Types

KTP v1 reserves these frame type IDs:

| ID | Name | Direction | Session ID | Purpose |
| --- | --- | --- | --- | --- |
| `0x01` | `HELLO` | agent -> backend | `0` | Register data-plane protocol and capabilities. |
| `0x02` | `HELLO_ACK` | backend -> agent | `0` | Confirm accepted protocol and connection settings. |
| `0x03` | `READY` | agent -> backend | `0` | Report rule revision and data-plane ready rules. |
| `0x10` | `SESSION_OPEN` | ingress -> backend, backend -> egress | non-zero | Create one TCP tunnel session. |
| `0x11` | `SESSION_ACCEPT` | egress -> backend, backend -> ingress | non-zero | Confirm target connection is established. |
| `0x12` | `SESSION_DATA` | bidirectional | non-zero | Carry TCP bytes. |
| `0x13` | `SESSION_WINDOW` | bidirectional | non-zero | Grant flow-control credit. |
| `0x14` | `SESSION_CLOSE` | bidirectional | non-zero | Close the session normally. |
| `0x15` | `SESSION_ERROR` | bidirectional | non-zero | Close the session with a typed error. |
| `0x20` | `PING` | bidirectional | `0` | Keepalive and latency measurement. |
| `0x21` | `PONG` | bidirectional | `0` | Reply to `PING`. |
| `0x30` | `STATS` | agent -> backend | `0` | Report counters and active sessions. |

## Payloads

Payloads are binary encoded in v1 using a simple length-prefixed format:

- strings are `u16 length` plus UTF-8 bytes.
- byte arrays are `u32 length` plus raw bytes.
- repeated lists are `u16 count` plus repeated items.
- integers use network byte order.

This avoids a JSON data path while keeping v1 easy to implement and test.

### `HELLO`

Fields:

- agent UUID.
- agent version.
- KTP version.
- last known rule revision.
- capability list:
  - `tcp`.
  - `multiplex`.
  - `flow_control`.
  - `stats`.
- max accepted frame size.
- max concurrent sessions supported by this agent.

Backend accepts `HELLO` only after token authentication has already identified
the same client UUID and user ID.

### `HELLO_ACK`

Fields:

- accepted KTP version.
- backend relay ID.
- heartbeat interval seconds.
- max frame size.
- initial window bytes per session.
- idle timeout seconds.

### `READY`

Fields:

- rule revision.
- ready rule IDs for ingress role.
- ready rule IDs for egress role.
- failed rule statuses:
  - rule ID.
  - status.
  - error string.

`READY` connects the Phase 2 control plane to data-plane readiness. A rule is
not eligible for relay pairing until the backend has both:

- a current control-plane rule revision from Phase 2.
- a data-plane `READY` for the relevant role.

### `SESSION_OPEN`

Ingress to backend fields:

- rule ID.
- rule revision.
- source IP.
- source port.
- requested protocol, initially `tcp`.

Backend to egress fields:

- rule ID.
- rule revision.
- target host from database.
- target port from database.
- ingress client UUID.
- source IP and port for observability.

The ingress agent cannot choose the egress target. The backend must fill target
fields from the authorized rule.

### `SESSION_ACCEPT`

Fields:

- accepted session ID.
- egress client UUID.
- optional target local address.

### `SESSION_DATA`

Payload:

- raw TCP bytes.

Data frames are flow-controlled per session. A sender may not exceed its current
credit window.

### `SESSION_WINDOW`

Fields:

- additional allowed bytes.

The MVP uses a conservative per-session window. Later phases can tune it after
real traffic tests.

### `SESSION_CLOSE`

Fields:

- close code.
- reason string.
- bytes sent.
- bytes received.

### `SESSION_ERROR`

Fields:

- error code.
- reason string.

Initial error codes:

- `auth_failed`
- `rule_not_found`
- `revision_mismatch`
- `ingress_not_ready`
- `egress_not_ready`
- `no_egress_available`
- `source_denied`
- `session_limit`
- `target_failed`
- `listen_failed`
- `protocol_error`
- `relay_unavailable`
- `timeout`

## Session Lifecycle

1. Agent establishes the data WebSocket.
2. Agent sends `HELLO`.
3. Backend validates token, user feature, client UUID, control capability, and
   recent Phase 2 heartbeat.
4. Backend sends `HELLO_ACK`.
5. Agent sends `READY`.
6. Ingress agent accepts a local TCP connection.
7. Ingress sends `SESSION_OPEN` with `leg=1`.
8. Backend validates rule ownership, role, revision, source allowlist, and
   limits.
9. Backend selects one egress data connection using round robin among ready
   egress members for the same user and rule revision.
10. Backend forwards `SESSION_OPEN` to egress with `leg=2`.
11. Egress dials the backend-supplied target.
12. Egress sends `SESSION_ACCEPT` or `SESSION_ERROR` with `leg=2`.
13. Backend forwards accept/error to ingress with `leg=1`.
14. Both sides exchange `SESSION_DATA` through backend relay.
15. Either side sends `SESSION_CLOSE`, or backend closes on timeout/error.
16. Backend updates counters and recent rule/member status.

## Egress Selection

The first MVP uses per-rule round robin:

- only clients in the rule egress group are eligible.
- only clients owned by the same user are eligible.
- only clients with matching current rule revision are eligible.
- only clients with data-plane `READY` for egress or both are eligible.
- disconnected, stale, unsupported, or erroring egress clients are skipped.

If no egress is available, backend sends `SESSION_ERROR` with
`no_egress_available`.

## Security Boundaries

Backend enforcement:

- Token middleware binds the data connection to one `user_id` and `client_uuid`.
- The `tunnels` feature must be enabled for the user.
- The client must have recently registered tunnel control capability.
- Rules are loaded by `user_id` and rule ID.
- Agents cannot open sessions for rules not present in their role-specific
  rule sync.
- Ingress and egress clients must belong to the same user.
- Backend chooses egress target host and port from the database.
- Backend enforces current revision and session limits.
- Backend logs high-level session metadata without storing payload bytes.

Agent enforcement:

- Ingress binds only rules received from backend for ingress or both.
- Ingress enforces source allowlist before `SESSION_OPEN`.
- Egress dials only target host and port supplied by backend for an authorized
  session.
- Rule or session failures are reported as tunnel status, not process failures.

Cryptography boundary:

- KTP is not encryption.
- MVP encryption is provided by HTTPS/WSS termination.
- Future raw TLS mode must use a standard TLS implementation such as rustls.
- The project must not design custom cryptography.

## Resource Limits

The MVP must define limits before runtime code is enabled:

- max active rules per agent.
- max active sessions per rule.
- max active sessions per agent.
- max frame payload size.
- max buffered bytes per session.
- initial flow-control window.
- listener bind timeout.
- target dial timeout.
- session idle timeout.
- data connection heartbeat timeout.
- reconnect backoff with jitter.

When a limit is reached, only the affected session or rule fails.

## Status Model

Data-plane status builds on Phase 2 status strings:

- `ok`: ingress listeners are ready and at least one egress is ready.
- `partial`: some members are unhealthy but the rule can still serve traffic.
- `listen_failed`: one or more ingress members failed to bind.
- `relay_unavailable`: data connection to backend relay is unavailable.
- `target_failed`: egress target dial failed recently.
- `auth_failed`: data-plane authorization failed.

Phase 3 runtime code should eventually preserve per-member details, but the MVP
can first aggregate recent data-plane error summaries into existing rule status
views.

## MVP Scope

The implementation that follows this design should first deliver only:

- KTP frame encode/decode library.
- backend data endpoint skeleton that authenticates and validates `HELLO`.
- agent data runtime skeleton that can connect and perform `HELLO` / `HELLO_ACK`
  / `READY`.
- no TCP listeners.
- no target dials.
- no `SESSION_DATA` forwarding.
- tests for protocol, authorization, state transitions, and non-interference.

The next implementation slice after that can add TCP ingress listeners and fake
relay tests. Real RDP smoke testing should wait until the skeleton is stable.

## Testing Plan

### KTP protocol tests

- Encodes and decodes every frame type.
- Rejects wrong magic.
- Rejects unsupported version.
- Rejects unknown frame type.
- Rejects invalid leg for frame type.
- Rejects truncated headers and payloads.
- Rejects payload length above limit.
- Preserves `session_id`, `leg`, and payload bytes exactly.
- Verifies one WebSocket binary message maps to one KTP frame.

### Backend tests

- `/api/clients/tunnel/data` rejects non-WebSocket requests.
- Data endpoint rejects text WebSocket messages.
- Data endpoint rejects invalid token.
- Data endpoint rejects users without `tunnels`.
- Data endpoint rejects `HELLO` when Phase 2 control capability is missing.
- Data endpoint accepts `HELLO` for a current tunnel-capable client.
- `READY` updates data-plane readiness without changing server-page online
  status.
- Ingress `SESSION_OPEN` cannot select target host or port.
- Backend selects egress only inside the same `user_id`.
- Backend round robin skips stale or disconnected egress clients.
- Same-machine loopback keeps distinct ingress and egress legs for one
  `session_id`.

### Agent tests

- Data runtime is disabled when tunnel feature config is off.
- Data runtime failure does not stop report, exec, ping, or terminal tests.
- Agent sends `HELLO` with KTP capabilities.
- Agent sends `READY` only for rules from the current control revision.
- Ingress listener planning does not bind ports in skeleton phase.
- Invalid backend frames close only the tunnel data connection.
- Same-agent ingress/egress demux uses `leg` and `session_id`.

### Integration smoke tests

The first integration smoke after skeleton implementation should use fake local
TCP endpoints, not real RDP:

- one agent data connection can handshake with backend.
- two fake agents can be paired for a fake session.
- same fake agent can represent both ingress and egress legs.
- existing metrics/report loop remains alive while tunnel data connection
  reconnects.

Real RDP-style smoke comes later:

```text
machine-1 group :10088 -> backend relay -> machine-1 group -> 127.0.0.1:3389
```

and:

```text
ingress group :10088 -> backend relay -> egress group -> 127.0.0.1:3389
```

## Rollout Path

1. KTP frame library and tests.
2. Data endpoint skeleton and agent data skeleton.
3. Data-plane readiness reporting.
4. Ingress listener manager with no relay forwarding.
5. Fake relay session pairing tests.
6. TCP target dial and bidirectional `SESSION_DATA`.
7. Flow-control tuning and counters.
8. RDP smoke tests.
9. Optional raw TLS carrier.

## Success Criteria

This design phase is complete when:

- the first carrier is clearly chosen as WebSocket binary over HTTPS.
- KTP frame structure is defined.
- ingress, egress, and same-machine loopback roles are defined.
- backend relay responsibilities are defined.
- rule and user security boundaries are explicit.
- MVP scope excludes real data forwarding code.
- a concrete test plan exists for protocol, backend, agent, and integration.
