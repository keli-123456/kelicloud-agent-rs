# KTP E2E Runs Statistics Design

## Goal

Add repeated-run statistics to `ktp-e2e-bench` so KTP runtime performance work
can compare min, median, and max samples instead of relying on one wall-clock
measurement.

This advances the Linux-only high-performance KTP data-plane goal by making
the benchmark evidence less sensitive to run-to-run variance. The phase changes
benchmark tooling and documentation only. It must not change production tunnel
runtime behavior, KTP frame compatibility, backend relay behavior, or existing
agent monitoring/task/terminal paths.

## Current State

The project has two benchmark binaries:

- `ktp-tunnel-bench` already supports `--runs` and reports aggregate repeated
  carrier measurements.
- `ktp-e2e-bench` supports `--clients`, `--frames`, and `--payload-bytes`, but
  reports only one runtime ingress-to-egress sample.

Linux measurements show visible variance on the current small host. The next
performance optimization needs repeated e2e samples before runtime scheduling
changes are judged.

## Scope

Included:

- Add `--runs N` to `ktp-e2e-bench`.
- Default `runs` to `1`.
- Reject `--runs 0` with the same positive integer validation style used by
  existing flags.
- Run the existing e2e benchmark body `N` times.
- Output `runs=<N>`.
- For `runs=1`, keep existing fields and add `runs=1`.
- For `runs>1`, output:
  - `elapsed_ms_min`
  - `elapsed_ms_median`
  - `elapsed_ms_max`
  - `throughput_mib_s_min`
  - `throughput_mib_s_median`
  - `throughput_mib_s_max`
- Keep `bytes` as bytes per run, because every run uses the same clients,
  frames, and payload size.
- Update benchmark documentation after Linux release evidence is collected.

Excluded:

- Production runtime scheduling changes.
- New KTP frame fields.
- Raw TLS or carrier changes.
- Latency percentile histograms.
- UI changes.

## Output Semantics

Single run remains compact:

```text
ktp_e2e_bench ... runs=1 clients=1 frames=... bytes=... elapsed_ms=... throughput_mib_s=...
```

Multiple runs use summary fields:

```text
ktp_e2e_bench ... runs=3 clients=4 frames=... bytes=... elapsed_ms_min=... elapsed_ms_median=... elapsed_ms_max=... throughput_mib_s_min=... throughput_mib_s_median=... throughput_mib_s_max=...
```

Median for an even number of runs should be the average of the two middle
sorted values. This avoids surprising jumps when the user selects `--runs 2`.

Throughput summary sorts throughput values independently. Since higher
throughput is better, `throughput_mib_s_min` is the slowest run and
`throughput_mib_s_max` is the fastest run.

## Tests

TDD implementation should add failing tests before production code:

- Existing default CLI test expects `runs=1`.
- New CLI test with `--runs 2` expects:
  - `runs=2`
  - `bytes=<per-run bytes>`
  - `elapsed_ms_min=`
  - `elapsed_ms_median=`
  - `elapsed_ms_max=`
  - `throughput_mib_s_min=`
  - `throughput_mib_s_median=`
  - `throughput_mib_s_max=`
- New CLI validation test rejects `--runs 0`.

Existing tunnel runtime and benchmark tests must continue to pass.

## Linux Evidence

After implementation, collect release-mode repeated e2e samples on the Linux
host:

```bash
./target/release/ktp-e2e-bench --runs 3 --clients 1 --frames 1024 --payload-bytes 1024
./target/release/ktp-e2e-bench --runs 3 --clients 4 --frames 256 --payload-bytes 1024
./target/release/ktp-e2e-bench --runs 3 --clients 4 --frames 256 --payload-bytes 16384
```

Record these rows in `docs/ktp-benchmarks.md` with the commit hash.

## Rollback

Rollback is a benchmark-only revert. Default `runs=1` keeps existing command
behavior compatible except for the additional `runs=1` field in stdout.
