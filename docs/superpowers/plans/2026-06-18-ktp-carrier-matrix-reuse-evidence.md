# KTP Carrier Matrix Reuse Evidence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve `write_batch_reused=1` in carrier matrix CSV output so release evidence proves batch-write samples avoid timed payload cloning.

**Architecture:** Keep `ktp-tunnel-bench` and the KTP carrier unchanged. Extend `scripts/ktp-carrier-matrix.sh` to parse the optional `write_batch_reused` metric, default it to `0` for directions that do not emit it, and write it into the CSV next to `write_batch_frames`.

**Tech Stack:** Bash matrix script, existing `ktp-tunnel-bench` key-value output, Cargo integration tests.

---

## File Structure

- Modify `scripts/ktp-carrier-matrix.sh`
  - Add `write_batch_reused` to the CSV header and rows.
  - Default missing values to `0`.
- Modify `tests/ktp_carrier_matrix_script.rs`
  - Assert the script contains and writes `write_batch_reused`.
  - Make the fake cargo output emit `write_batch_reused=1` for `client-to-relay-batch-write`.
- Modify `docs/ktp-benchmarks.md`
  - Update the carrier matrix CSV sample with the new column.

## Task 1: Red Test

- [ ] **Step 1: Add failing CSV assertions**

Update `tests/ktp_carrier_matrix_script.rs` so the expected header is:

```text
direction,runs,frames,payload_bytes,write_batch_frames,write_batch_reused,read_batch_frames,...
```

The expected batch-write row should include `64,1,0` for
`write_batch_frames,write_batch_reused,read_batch_frames`.

- [ ] **Step 2: Run the test**

```powershell
cargo test --test ktp_carrier_matrix_script -- --nocapture
```

Expected: FAIL because the script does not parse or write `write_batch_reused`.

## Task 2: Implementation

- [ ] **Step 1: Update matrix script**

Parse `write_batch_reused` from benchmark output with `metric_value`, default it
to `0`, then add it to `csv_header` and `write_csv_row`.

- [ ] **Step 2: Run focused tests**

```powershell
cargo test --test ktp_carrier_matrix_script -- --nocapture
```

Expected: PASS.

## Task 3: Evidence And Commit

- [ ] **Step 1: Run Linux matrix script test**

Run the same integration test on the Linux host so the fake cargo CSV path is
exercised under Bash.

- [ ] **Step 2: Update docs**

Refresh `docs/ktp-benchmarks.md` CSV header and rows to include
`write_batch_reused`.

- [ ] **Step 3: Commit**

```powershell
git add scripts/ktp-carrier-matrix.sh tests/ktp_carrier_matrix_script.rs docs/ktp-benchmarks.md docs/superpowers/plans/2026-06-18-ktp-carrier-matrix-reuse-evidence.md
git commit -m "bench: record ktp carrier batch reuse in matrix"
```
