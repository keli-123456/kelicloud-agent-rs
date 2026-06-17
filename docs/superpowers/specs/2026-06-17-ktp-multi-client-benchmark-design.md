# KTP Multi-Client Benchmark Design

## Goal

Add repeatable multi-client end-to-end benchmark coverage for the KTP runtime
path in `kelicloud-agent-rs`.

This is the next evidence step for the Linux-only high-performance KTP data
plane. The existing single-client benchmark proves that one ingress session can
flow through ingress runtime, KTP data frames, egress runtime, and a TCP echo
target. It does not show how the runtime behaves when several sessions are
active at the same time.

The phase adds measurement capability only. It must not change production
tunnel behavior, KTP frame compatibility, existing agent reporting, task
execution, terminal, metrics, or rule sync paths.

## Current State

The project currently has:

- `src/bin/ktp-e2e-bench.rs`, a single-client end-to-end benchmark.
- `src/bin/ktp-tunnel-bench.rs`, an encrypted carrier benchmark.
- `docs/ktp-benchmarks.md`, which records Linux release baseline numbers.
- `TunnelTcpRuntime::next_client_frames`, which lets the data carrier drain
  outbound runtime frames in batches.
- Runtime tests that cover echo forwarding and 100 concurrent loopback sessions
  inside the async runtime layer.

Missing evidence:

- End-to-end throughput with more than one client TCP connection.
- A stable command shape for collecting multi-session benchmark data on Linux.
- Output fields that make client count and total payload volume explicit.

## Scope

Included:

- Add `--clients N` to `ktp-e2e-bench`.
- Default `--clients` to `1` so existing commands and tests keep working.
- Spawn `N` client TCP connections to the ingress listener.
- Accept and echo `N` target TCP connections.
- Relay all session frames until the expected total response bytes have passed
  through the egress-to-ingress direction.
- Report `clients=<N>` in benchmark output.
- Keep `frames=<N>` as per-client frame count.
- Keep `bytes=<TOTAL>` as aggregate payload bytes across all clients.
- Add tests proving CLI parsing and output for multi-client mode.
- Record Linux benchmark evidence after implementation.

Excluded:

- Production runtime scheduling changes.
- Backend relay changes.
- New KTP frame fields.
- Raw TLS carrier work.
- UDP, VPN, or TUN/TAP behavior.
- UI changes.

## Benchmark Semantics

Command shape:

```bash
ktp-e2e-bench --clients 4 --frames 256 --payload-bytes 1024
```

Definitions:

- `clients`: number of simultaneous TCP clients.
- `frames`: number of request/response payload writes per client.
- `payload_bytes`: size of each payload write.
- `bytes`: `clients * frames * payload_bytes`.
- `throughput_mib_s`: aggregate response throughput based on `bytes`.

The client traffic shape remains request/response per client:

```text
for each client:
  repeat frames times:
    write payload
    read payload-sized response
```

This preserves the current benchmark semantics while adding concurrent sessions.
It avoids turning the benchmark into a burst-write test unless a later phase
explicitly adds a separate pipelined mode.

## Relay Completion

The relay loop should continue until egress-to-ingress `SESSION_DATA` payload
bytes reach the aggregate expected byte count.

Counting bytes instead of data-frame count is mandatory because TCP may combine
or split reads. The benchmark must not assume that one client write becomes one
KTP `SESSION_DATA` frame.

The relay should keep using batched runtime drains:

```text
ingress_runtime.next_client_frames(RELAY_BATCH_FRAMES)
egress_runtime.next_client_frames(RELAY_BATCH_FRAMES)
```

This keeps the benchmark aligned with the production data-carrier path without
changing the production path in this phase.

## Error Handling

Invalid arguments should fail before starting sockets:

- `--clients 0` is rejected.
- Missing `--clients` value is rejected.
- Non-integer `--clients` value is rejected.
- Unknown arguments continue to be rejected.

Runtime benchmark errors should identify which high-level phase failed:

- client connection or I/O error.
- echo server accept or I/O error.
- relay timeout, including observed ingress bytes, egress bytes, and expected
  bytes.

## Tests

TDD implementation should add failing tests before production code:

- CLI test for default output containing `clients=1`.
- CLI test for `--clients 2` output containing `clients=2` and aggregate
  `bytes=clients * frames * payload_bytes`.
- Argument validation test for `--clients 0`.

Existing tests must continue to pass:

- `ktp_e2e_bench_cli`.
- `ktp_bench_cli`.
- `tunnel_async_runtime`.
- `tunnel_data`.
- `tunnel_runtime`.

## Linux Evidence

After implementation, collect release-mode benchmark data on the Linux host:

```bash
./target/release/ktp-e2e-bench --clients 1 --frames 1024 --payload-bytes 1024
./target/release/ktp-e2e-bench --clients 4 --frames 256 --payload-bytes 1024
./target/release/ktp-e2e-bench --clients 4 --frames 256 --payload-bytes 16384
```

Record the results in `docs/ktp-benchmarks.md` with the commit hash. Treat the
numbers as engineering baselines, not production capacity promises.

## Rollback

Rollback is simple because the phase touches benchmark tooling and documentation
only. If multi-client benchmarking behaves incorrectly, revert the benchmark
commit without changing production tunnel behavior.

The default `--clients 1` behavior must remain compatible with existing single
client benchmark commands.
