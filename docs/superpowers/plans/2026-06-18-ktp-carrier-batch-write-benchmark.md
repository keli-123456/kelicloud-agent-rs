# KTP Carrier Batch Write Benchmark Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add repeatable benchmark coverage for encrypted KTP TCP batch writes from client to relay.

**Architecture:** Keep the existing KTP encrypted TCP carrier unchanged. Extend `ktp-tunnel-bench` with a new `client-to-relay-batch-write` direction that sends KTP frames through `KtpEncryptedTcpStream::send_frames`, then extend the carrier matrix CSV so batch-write and batch-read paths can be compared from the same script.

**Tech Stack:** Rust 2021, Tokio TCP, existing KTP encrypted TCP carrier, Bash matrix script, Cargo integration tests.

---

## File Structure

- Modify `src/bin/ktp-tunnel-bench.rs`
  - Add `client-to-relay-batch-write` as a benchmark direction.
  - Emit `write_batch_frames=64` for the new direction.
- Modify `scripts/ktp-carrier-matrix.sh`
  - Include the new direction in default matrix coverage.
  - Add `write_batch_frames` to CSV output.
- Modify `tests/ktp_bench_cli.rs`
  - Add a CLI regression test for the new direction.
- Modify `tests/ktp_carrier_matrix_script.rs`
  - Add script and CSV assertions for the new direction and column.
- Modify `docs/ktp-benchmarks.md`
  - Document the new carrier matrix field and why it exists.

## Task 1: CLI Direction

- [ ] **Step 1: Write failing CLI test**

Add `ktp_tunnel_bench_cli_reports_client_to_relay_batch_write` to
`tests/ktp_bench_cli.rs`. The test runs:

```powershell
cargo test --test ktp_bench_cli ktp_tunnel_bench_cli_reports_client_to_relay_batch_write -- --nocapture
```

Expected: FAIL because `client-to-relay-batch-write` is not yet accepted.

- [ ] **Step 2: Implement the bench direction**

Update `BenchDirection`, `parse`, `report_value`, `batch_read_suffix`, and
`run_benchmark_once` in `src/bin/ktp-tunnel-bench.rs`. The implementation should
reuse `KtpEncryptedTcpStream::send_frames` with a fixed batch size of 64 frames
and report `write_batch_frames=64`.

- [ ] **Step 3: Verify CLI test passes**

Run:

```powershell
cargo test --test ktp_bench_cli ktp_tunnel_bench_cli_reports_client_to_relay_batch_write -- --nocapture
```

Expected: PASS.

## Task 2: Carrier Matrix CSV

- [ ] **Step 1: Write failing matrix tests**

Update `tests/ktp_carrier_matrix_script.rs` to require the new default direction,
`write_batch_frames`, and CSV row parsing from fake cargo output.

Run:

```powershell
cargo test --test ktp_carrier_matrix_script -- --nocapture
```

Expected: FAIL because the script does not include the direction or column yet.

- [ ] **Step 2: Update matrix script**

Modify `scripts/ktp-carrier-matrix.sh` so defaults include:

```bash
client-to-relay client-to-relay-batch-write relay-to-client-batch-read
```

Add `write_batch_frames` to the CSV header and row extraction, defaulting to `0`
when the bench output omits it.

- [ ] **Step 3: Verify matrix tests pass**

Run:

```powershell
cargo test --test ktp_carrier_matrix_script -- --nocapture
```

Expected: PASS.

## Task 3: Verification And Evidence

- [ ] **Step 1: Run focused Rust tests**

```powershell
cargo test --test ktp_bench_cli --test ktp_carrier_matrix_script -- --nocapture
cargo test --test ktp_transport -- --nocapture
```

- [ ] **Step 2: Run Linux dry-run and small release sample**

On the Linux test host, run a dry-run for all default directions and a small
release sample for `client-to-relay-batch-write`.

- [ ] **Step 3: Update benchmark notes**

Record that the carrier matrix now compares one-by-one client writes,
client-side batch writes, and relay-to-client batch reads. Do not claim a
throughput win unless the release sample actually proves one.

- [ ] **Step 4: Commit**

```powershell
git add src/bin/ktp-tunnel-bench.rs scripts/ktp-carrier-matrix.sh tests/ktp_bench_cli.rs tests/ktp_carrier_matrix_script.rs docs/ktp-benchmarks.md docs/superpowers/plans/2026-06-18-ktp-carrier-batch-write-benchmark.md
git commit -m "bench: add ktp carrier batch write direction"
```
