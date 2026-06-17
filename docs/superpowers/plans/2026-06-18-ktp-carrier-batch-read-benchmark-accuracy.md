# KTP Carrier Batch Read Benchmark Accuracy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the encrypted KTP TCP relay-to-client batch-read benchmark measure carrier batch delivery instead of per-chunk payload cloning overhead.

**Architecture:** Keep the carrier, crypto record format, and production tunnel runtime unchanged. Refactor `ktp-tunnel-bench` so `relay-to-client-batch-read` prebuilds reusable frame batches before the timed client read loop, then reports `read_batch_reused=1`. Extend the carrier matrix CSV to preserve that evidence.

**Tech Stack:** Rust 2021, existing `ktp-tunnel-bench`, Bash carrier matrix script, Cargo integration tests.

---

## File Structure

- Modify `src/bin/ktp-tunnel-bench.rs`
  - Prebuild reusable relay-to-client frame batches before spawning the sender.
  - Add `read_batch_reused=1` to the `relay-to-client-batch-read` report suffix.
- Modify `tests/ktp_bench_cli.rs`
  - Assert that the batch-read direction reports `read_batch_reused=1`.
- Modify `scripts/ktp-carrier-matrix.sh`
  - Add optional `read_batch_reused` to CSV output next to `read_batch_frames`.
- Modify `tests/ktp_carrier_matrix_script.rs`
  - Assert the new CSV column and fake cargo row values.
- Modify `docs/ktp-benchmarks.md`
  - Update the carrier matrix sample and note both batch directions use reusable prebuilt batches.

## Task 1: Bench Contract

- [ ] **Step 1: Write failing CLI assertion**

Update `tests/ktp_bench_cli.rs`:

```rust
assert!(stdout.contains("read_batch_reused=1"));
```

Run:

```powershell
cargo test --test ktp_bench_cli ktp_tunnel_bench_cli_reports_relay_to_client_batch_read -- --nocapture
```

Expected: FAIL because the report only includes `read_batch_frames=64`.

- [ ] **Step 2: Implement reusable batch-read batches**

Use the existing reusable batch helper to build relay-to-client batches before
the sender loop. The timed client side should only wait for and decode incoming
frames.

- [ ] **Step 3: Verify focused CLI test**

```powershell
cargo test --test ktp_bench_cli ktp_tunnel_bench_cli_reports_relay_to_client_batch_read -- --nocapture
```

Expected: PASS.

## Task 2: Matrix Evidence

- [ ] **Step 1: Write failing matrix assertions**

Update `tests/ktp_carrier_matrix_script.rs` to require
`read_batch_reused` in the CSV header. The relay-to-client row should contain
`0,0,64,1` for `write_batch_frames,write_batch_reused,read_batch_frames,read_batch_reused`.

- [ ] **Step 2: Implement script parsing**

Parse optional `read_batch_reused`, defaulting to `0`, and write it into the
CSV.

- [ ] **Step 3: Verify focused matrix test**

```powershell
cargo test --test ktp_carrier_matrix_script -- --nocapture
```

Expected: PASS.

## Task 3: Evidence And Commit

- [ ] **Step 1: Run focused and full tests**

```powershell
cargo test --test ktp_bench_cli -- --nocapture
cargo test --test ktp_carrier_matrix_script -- --nocapture
cargo test --tests --quiet
```

- [ ] **Step 2: Run Linux release smoke**

Run the carrier matrix on Linux with the same small shape and refresh
`docs/ktp-benchmarks.md` with the new CSV values.

- [ ] **Step 3: Commit**

```powershell
git add src/bin/ktp-tunnel-bench.rs tests/ktp_bench_cli.rs scripts/ktp-carrier-matrix.sh tests/ktp_carrier_matrix_script.rs docs/ktp-benchmarks.md docs/superpowers/plans/2026-06-18-ktp-carrier-batch-read-benchmark-accuracy.md
git commit -m "bench: reuse ktp carrier batch read frames"
```
