# Linux Metrics Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Align the Rust Linux agent's core metric calculations with the current Go agent for BasicInfo and Report payloads.

**Architecture:** Keep Linux parsing in `src/linux_proc.rs` as pure testable helpers. Keep runtime orchestration in `src/system.rs`, using Linux helpers first and `sysinfo` as fallback.

**Tech Stack:** Rust 2021, `sysinfo`, `/proc` fixtures, cargo integration tests.

---

### Task 1: RAM and Swap Parity

**Files:**
- Modify: `src/linux_proc.rs`
- Modify: `src/system.rs`
- Modify: `tests/linux_proc.rs`
- Modify: `tests/system.rs`

- [ ] Add a failing `/proc/meminfo` fixture test for Go-compatible RAM and swap byte calculations.
- [ ] Run `cargo test --test linux_proc parse_meminfo_calculates_go_compatible_memory`.
- [ ] Add `ProcMemInfo`, `parse_meminfo`, `go_compatible_ram`, and `go_compatible_swap` helpers.
- [ ] Run the targeted test and confirm it passes.
- [ ] Wire `SystemSnapshotCollector` to prefer Linux meminfo values over `sysinfo` memory values.
- [ ] Add a snapshot mapping test proving byte values pass through unchanged.

### Task 2: Disk Filtering Parity

**Files:**
- Modify: `src/linux_proc.rs`
- Modify: `src/system.rs`
- Modify: `tests/linux_proc.rs`

- [ ] Add failing tests for physical disk filtering: include `/`, exclude tmpfs/overlay/docker paths, exclude `/dev/loop*`, and deduplicate ZFS by pool.
- [ ] Run `cargo test --test linux_proc disk_mounts_filter_like_go_agent`.
- [ ] Implement small mount filtering helpers in `linux_proc`.
- [ ] Wire runtime disk collection through the parity filter where available.
- [ ] Run targeted tests and confirm they pass.

### Task 3: BasicInfo Enrichment

**Files:**
- Modify: `src/linux_proc.rs`
- Modify: `src/system.rs`
- Modify: `tests/linux_proc.rs`
- Modify: `tests/system.rs`

- [ ] Add failing tests for CPU name fallback from `/proc/cpuinfo`.
- [ ] Add failing tests for virtualization fallback from container markers/cgroup text.
- [ ] Extend `SystemSnapshot` with `ipv4`, `ipv6`, and `virtualization` values that map into BasicInfo.
- [ ] Implement Linux-local IP fallback from non-loopback interfaces when public IP probing is unavailable.
- [ ] Run targeted tests and confirm they pass.

### Task 4: Verification and Docs

**Files:**
- Modify: `README.md`

- [ ] Update README to list Linux metric parity details and remaining exclusions.
- [ ] Run `cargo fmt --check`.
- [ ] Run `cargo test`.
- [ ] Inspect the result against the design scope before marking the goal complete.
