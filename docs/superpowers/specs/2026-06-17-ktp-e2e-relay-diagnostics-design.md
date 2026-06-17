# KTP E2E Relay Diagnostics Design

## Goal

Add optional relay-loop diagnostics to `ktp-e2e-bench` so small-frame runtime
performance can be investigated before changing production scheduling logic.

This phase is diagnostic only. It changes benchmark tooling and benchmark
documentation, not the production tunnel runtime, KTP frame format, backend
relay, or existing agent monitor/task/terminal behavior.

## Current State

`ktp-e2e-bench` now supports:

- `--clients N`
- `--runs N`
- repeated-run min/median/max summaries

The benchmark still reports only wall-clock time and throughput. For small
frames, those numbers do not show whether time is spent in:

- empty relay-loop turns,
- repeated `thread::yield_now()` calls,
- too many tiny KTP frames,
- ingress-to-egress handling,
- egress-to-ingress handling.

## Design

Add an optional boolean flag:

```bash
ktp-e2e-bench --diagnostics --clients 4 --frames 256 --payload-bytes 1024
```

Default output stays unchanged when `--diagnostics` is not present.

When diagnostics are enabled, each report appends aggregate relay counters:

- `relay_turns`: relay loop iterations.
- `relay_empty_turns`: loop iterations where neither runtime produced frames.
- `relay_yield_turns`: loop iterations that called `thread::yield_now()`.
- `ingress_frames`: all frames drained from ingress runtime.
- `egress_frames`: all frames drained from egress runtime.
- `ingress_data_frames`: ingress `SESSION_DATA` frames.
- `egress_data_frames`: egress `SESSION_DATA` frames.

For repeated runs, diagnostics should be totals across all runs. The timing
fields remain min/median/max as currently implemented.

## Non-Goals

- No production runtime scheduling changes.
- No sleeps, condition variables, async notifications, or queue behavior changes.
- No new KTP frame fields.
- No latency histogram in this phase.
- No backend changes.

## Tests

Add CLI tests before implementation:

- `--diagnostics` output contains the diagnostic fields.
- Default output does not contain diagnostic fields.

Existing CLI, runtime, and tunnel data tests must keep passing.

## Linux Evidence

After implementation, run:

```bash
./target/release/ktp-e2e-bench --diagnostics --runs 3 --clients 1 --frames 1024 --payload-bytes 1024
./target/release/ktp-e2e-bench --diagnostics --runs 3 --clients 4 --frames 256 --payload-bytes 1024
./target/release/ktp-e2e-bench --diagnostics --runs 3 --clients 4 --frames 256 --payload-bytes 16384
```

Record the output in `docs/ktp-benchmarks.md` as diagnostic evidence. The
purpose is to guide the next scheduling optimization, not to claim production
capacity.

## Rollback

Rollback is a benchmark-only revert. Since `--diagnostics` is opt-in, existing
benchmark commands remain compatible.
