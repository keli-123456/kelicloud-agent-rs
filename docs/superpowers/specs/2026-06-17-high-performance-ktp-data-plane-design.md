# High Performance KTP Data Plane Design

## Goal

Evolve the current tunnel forwarding MVP into a Linux-only high-performance
KTP data plane without breaking existing agent behavior. The first phase builds
the performance foundation inside `kelicloud-agent-rs` while keeping the
current backend relay and WebSocket binary carrier compatible.

This phase makes the requested final state more true by replacing the current
blocking thread-per-session TCP runtime with an async, bounded, measurable data
runtime. Raw TLS and later transport work remain follow-up phases once the
runtime has clear resource limits and regression coverage.

## Current State

The project already has a project-owned KTP frame protocol:

- Rust codec: `src/ktp.rs`.
- Rust tunnel data WebSocket session: `src/tunnel_data.rs`.
- Rust TCP runtime: `src/tunnel_runtime.rs`.
- Go backend KTP codec and data endpoint:
  `api/client/tunnel_data_protocol.go` and `api/client/tunnel_data.go`.
- Go backend relay pairing: `api/client/tunnel_relay.go`.

Current data forwarding works, but the agent-side TCP runtime uses
`std::net`, blocking reads and writes, OS threads, unbounded-ish frame queues,
and `std::sync::mpsc`. That is acceptable for an MVP, but it is not a good
foundation for many concurrent sessions, predictable memory use, or future raw
TLS/QUIC carriers.

## First-Phase Scope

The first phase is `KTP async runtime foundation`.

Included:

- Linux-only Tokio runtime for tunnel TCP listeners and target connections.
- Bounded channels between TCP sessions and the KTP carrier.
- Per-session buffer limits and deterministic close/error behavior.
- Session lifecycle cleanup for local close, remote close, target error, and
  relay disconnect.
- Runtime stats snapshots that can later feed KTP `STATS` and the web UI.
- Compatibility with the current KTP frame format and WebSocket data endpoint.
- Tests proving the report/control/terminal code paths are not required for
  the tunnel runtime and are not stopped by tunnel failures.

Excluded from this phase:

- Raw TLS carrier.
- QUIC carrier.
- UDP forwarding.
- VPN/TUN behavior.
- Custom cryptography.
- Backend relay replacement or standalone relay service.
- UI redesign.

## Architecture

The new data plane is split into three Rust units:

1. `ktp` frame codec
   - Remains the stable protocol boundary.
   - Adds no transport assumptions.

2. `tunnel_runtime`
   - Owns rule application, TCP listeners, target dials, session workers, and
     runtime stats.
   - Moves from blocking thread workers to Tokio tasks.
   - Exposes a small compatibility adapter through the existing
     `TunnelSessionRuntime` trait so `src/tunnel_data.rs` can continue driving
     frames through the current carrier.

3. `tunnel_data`
   - Keeps the current WebSocket carrier in phase one.
   - Continues to handshake with `HELLO`, read `HELLO_ACK`, publish `READY`,
     and pass KTP session frames to the runtime.
   - Does not own TCP session internals.

The compatibility boundary is intentional. It lets this phase improve the data
runtime without forcing a backend protocol migration in the same step.

## Async Runtime Model

Every tunnel rule that needs ingress support creates one Tokio listener task.
Every accepted TCP connection creates a session with:

- One task reading local TCP bytes into bounded KTP outbound frames.
- One task writing inbound KTP bytes to the local TCP stream.
- A shared session record containing rule ID, session ID, leg, created time,
  byte counters, and close reason.

Egress sessions follow the same model after the agent receives a KTP
`SESSION_OPEN` from the relay and successfully dials the configured target.

The KTP carrier sees only frames:

- `SESSION_OPEN` when ingress accepts a new source connection.
- `SESSION_ACCEPT` when egress target dial succeeds.
- `SESSION_DATA` for byte chunks.
- `SESSION_CLOSE` for clean EOF or local shutdown.
- `SESSION_ERROR` for typed failures.

## Backpressure And Limits

The async runtime must never allow an unbounded number of queued bytes or
sessions. Defaults for phase one:

- `max_sessions_per_rule`: existing rule value, with zero meaning unlimited by
  rule but still bounded by agent defaults.
- `max_sessions_per_agent`: 1024.
- `max_outbound_frames`: 4096.
- `max_session_pending_bytes`: 4 MiB.
- `tcp_read_chunk_size`: 16 KiB.
- `target_dial_timeout`: 5 seconds.
- `idle_timeout`: 10 minutes.
- `listener_accept_backoff`: 50 ms after recoverable accept errors.

When a bounded queue is full, the affected session closes with
`backpressure_limit` and the agent continues running. The listener and report
loop are not stopped.

## Compatibility

KTP frame compatibility is mandatory for this phase:

- Frame magic remains `KTP1`.
- Version remains `1`.
- Existing frame type IDs remain unchanged.
- One WebSocket binary message still carries one KTP frame.
- Existing backend tests for KTP encode/decode and relay routing must keep
  passing.

The runtime can add internal stats, but it must not require backend support for
new frame fields in this phase.

## Error Handling

Failures are scoped to the smallest affected object:

- Listener bind failure marks that rule's ingress side as failed.
- Target dial failure sends `SESSION_ERROR` for that session.
- Queue full sends `SESSION_ERROR` or `SESSION_CLOSE` for that session.
- Protocol decode error closes the tunnel data connection, then reconnects
  through the existing carrier loop.
- Carrier disconnect tears down active tunnel sessions, but does not stop
  metrics, task execution, ping, WebSSH, basic info upload, or auto-discovery.

All runtime errors should be typed strings already compatible with the current
diagnostic UI style, for example:

- `listen_bind_failed`.
- `target_connect_failed`.
- `backpressure_limit`.
- `session_limit`.
- `idle_timeout`.
- `runtime_unavailable`.

## Rollback Strategy

The change must be reversible:

- Keep the existing public trait boundary used by `tunnel_data`.
- Introduce the async runtime behind the same `TunnelSessionRuntime` behavior.
- Gate phase-one runtime activation with the existing
  `AGENT_TUNNEL_DATA_ENABLED` path.
- Keep no-rules behavior cheap: no listener tasks and no active session tasks.
- Make tests cover both readiness and data forwarding so regressions are caught
  before release.

If a production issue appears, disabling tunnel data or downgrading the agent
returns nodes to the previous monitoring-only behavior.

## Performance Evidence Required

This phase is not complete merely because the code compiles. It needs evidence:

- Unit tests for bounded queue behavior and session cleanup.
- Runtime tests for two-agent echo forwarding using the async runtime.
- A local benchmark or smoke command that reports throughput-oriented numbers
  for a loopback tunnel.
- A regression test showing unrelated agent functions are not coupled to the
  tunnel runtime.

Initial performance target:

- Maintain correct echo forwarding with at least 100 concurrent loopback
  sessions in the runtime test environment.
- Keep memory bounded by configured queue limits instead of growing with
  unlimited queued frames.

The 100-session target is a first engineering gate, not a marketing claim.
Later phases should add real host throughput and latency benchmarks before the
feature is described as production-grade high performance.

## Test Plan

Rust agent tests:

- KTP codec tests stay unchanged.
- Async runtime starts and stops listeners when rules change.
- Async runtime restarts listeners when listen address or port changes.
- Async runtime forwards echo traffic across simulated ingress and egress
  runtimes.
- Async runtime rejects new sessions when agent or rule limits are reached.
- Async runtime closes one session when its outbound queue is full.
- Async runtime removes session state after local close, remote close, and
  carrier disconnect.
- Existing tunnel data session tests continue to pass with the compatibility
  adapter.

Backend tests:

- Existing KTP and relay tests keep passing.
- No backend schema migration is required for this phase.
- Existing active session count behavior remains compatible.

Smoke tests:

- `scripts/tunnel-relay-local-smoke.sh` should run the async runtime relay
  simulation.
- Existing local backend smoke should still prove tunnel data can connect and
  relay echo.

## Future Phases

Phase two: raw TLS carrier.

- Add a dedicated KTP-over-rustls carrier.
- Keep WebSocket as compatibility mode.
- Add carrier negotiation and deployment documentation.

Phase three: stronger performance validation.

- Add real-host throughput and latency smoke tests.
- Add relay CPU and memory observations.
- Decide whether the backend relay should split into a dedicated relay service.

Phase four: optional QUIC carrier.

- Evaluate QUIC only after the runtime and raw TLS carrier are stable.
- Keep KTP as the framing/session layer unless evidence shows it should change.
