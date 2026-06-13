# Linux Metrics Parity Design

## Goal

Bring the Linux-only Rust agent's BasicInfo and Report metrics closer to the current Go agent behavior for the fields used by the server list and client detail views.

## Scope

This milestone covers core Linux server metrics only:

- RAM and swap totals/used values from `/proc/meminfo`, using the same byte units and formulas as the Go agent.
- Disk totals/used values with physical mount filtering similar to the Go agent.
- BasicInfo CPU name fallback, public/local IP fallback, and virtualization detection.
- Network totals/rates and TCP/UDP counts remain based on `/proc`, with the current interface exclusion behavior preserved.

This milestone does not cover GPU detailed metrics, vnstat month rotation, task execution, or terminal sessions.

## Data Flow

`SystemSnapshotCollector` continues to be the single runtime entry point. Linux-specific parsing stays in `linux_proc` as small pure functions that can be tested with fixture strings. Runtime collection uses Linux parsers first and keeps `sysinfo` as a fallback when a Linux file or command is unavailable.

## Compatibility Rules

- Memory values are bytes.
- RAM default used formula matches Go `GetMemHtopLike`:
  `MemTotal - (MemFree + Cached + SReclaimable + Buffers) + Shmem`, clamped to non-negative values.
- Swap used formula matches Go `Swap`:
  `SwapTotal - SwapFree - SwapCached`, with the same underflow fallback to `SwapTotal - SwapFree`.
- CPU usage is clamped to at least `0.001` in the report, matching the Go report behavior.
- Disk filtering keeps `/`, excludes temporary/container/network pseudo filesystems, excludes `/dev/loop*`, and deduplicates ZFS datasets by pool.
- Virtualization detection prefers `systemd-detect-virt`, then container markers/cgroup heuristics, then `none`.

## Testing

Tests should focus on deterministic pure parsing and mapping:

- `/proc/meminfo` fixture proves RAM and swap calculations.
- Mount fixture proves disk filtering and ZFS deduplication decisions.
- CPU info fixture proves CPU name fallback.
- Cgroup/container fixture proves virtualization fallback.
- Existing snapshot-to-report and snapshot-to-basic-info tests continue to prove payload shape.
