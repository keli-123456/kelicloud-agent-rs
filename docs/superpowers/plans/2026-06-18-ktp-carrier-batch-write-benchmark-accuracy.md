# KTP Carrier Batch Write Benchmark Accuracy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the encrypted KTP TCP batch-write benchmark measure carrier batching instead of per-chunk payload cloning overhead.

**Architecture:** Keep the carrier, crypto record format, and production tunnel runtime unchanged. Refactor `ktp-tunnel-bench` so `client-to-relay-batch-write` prebuilds reusable frame batches before timing starts, then reports `write_batch_reused=1` as evidence that the measurement is not cloning a fresh payload vector on every batch iteration.

**Tech Stack:** Rust 2021, existing `ktp-tunnel-bench`, existing KTP encrypted TCP stream, Cargo integration tests.

---

## File Structure

- Modify `src/bin/ktp-tunnel-bench.rs`
  - Add a small helper that builds reusable fixed-size frame batches before the timed section.
  - Add `write_batch_reused=1` to the `client-to-relay-batch-write` report suffix.
- Modify `tests/ktp_bench_cli.rs`
  - Assert that the batch-write direction reports `write_batch_reused=1`.
- Modify `docs/ktp-benchmarks.md`
  - Note that batch-write samples use prebuilt reusable batches.
  - Refresh the Linux smoke sample after running the updated benchmark.

## Task 1: Batch Write Report Contract

- [ ] **Step 1: Write failing CLI assertion**

Update `tests/ktp_bench_cli.rs`:

```rust
assert!(stdout.contains("write_batch_reused=1"));
```

Run:

```powershell
cargo test --test ktp_bench_cli ktp_tunnel_bench_cli_reports_client_to_relay_batch_write -- --nocapture
```

Expected: FAIL because the current report only includes `write_batch_frames=64`.

- [ ] **Step 2: Implement reusable batch generation**

In `src/bin/ktp-tunnel-bench.rs`, replace the timed loop's
`(0..chunk).map(|_| frame.clone()).collect::<Vec<_>>()` with a helper that
creates all needed batches before `Instant::now()`. The timed section should
iterate over `&[KtpFrame]` batches and call `send_frames(batch)`.

- [ ] **Step 3: Verify focused CLI test**

Run:

```powershell
cargo test --test ktp_bench_cli ktp_tunnel_bench_cli_reports_client_to_relay_batch_write -- --nocapture
```

Expected: PASS.

## Task 2: Evidence Update

- [ ] **Step 1: Run focused tests**

```powershell
cargo test --test ktp_bench_cli -- --nocapture
cargo test --test ktp_transport -- --nocapture
```

- [ ] **Step 2: Run Linux release smoke**

Run the carrier matrix on Linux with the same small shape as the previous smoke:

```bash
KTP_CARRIER_MATRIX_DIRECTIONS='client-to-relay client-to-relay-batch-write relay-to-client-batch-read' \
KTP_CARRIER_MATRIX_FRAMES=64 \
KTP_CARRIER_MATRIX_PAYLOAD_BYTES=1024 \
KTP_CARRIER_MATRIX_RUNS=2 \
KTP_CARRIER_MATRIX_CSV=/tmp/ktp-carrier-batch-write-reused-smoke.csv \
bash scripts/ktp-carrier-matrix.sh
```

- [ ] **Step 3: Refresh benchmark notes**

Update `docs/ktp-benchmarks.md` with the new CSV values. If batch-write remains
slower, keep saying so plainly.

- [ ] **Step 4: Commit**

```powershell
git add src/bin/ktp-tunnel-bench.rs tests/ktp_bench_cli.rs docs/ktp-benchmarks.md docs/superpowers/plans/2026-06-18-ktp-carrier-batch-write-benchmark-accuracy.md
git commit -m "bench: reuse ktp carrier batch write frames"
```
