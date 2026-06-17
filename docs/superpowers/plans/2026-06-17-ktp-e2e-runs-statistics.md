# KTP E2E Runs Statistics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `--runs N` to `ktp-e2e-bench` and report min, median, and max statistics for repeated KTP runtime e2e benchmark samples.

**Architecture:** Keep the production tunnel runtime untouched. Refactor the benchmark binary so one function returns a numeric sample and another formats either single-run output or repeated-run summaries. Reuse the existing positive integer parser and Cargo CLI tests.

**Tech Stack:** Rust 2021, standard library, existing `ktp-e2e-bench`, existing Cargo integration tests.

---

## File Structure

- Modify `tests/ktp_e2e_bench_cli.rs`
  - Adds CLI assertions for `runs=1`, `--runs 2`, and `--runs 0`.
- Modify `src/bin/ktp-e2e-bench.rs`
  - Adds `runs` to `BenchConfig`.
  - Parses `--runs`.
  - Refactors the benchmark body into samples and summary formatting.
- Modify `docs/ktp-benchmarks.md`
  - Records Linux repeated-run evidence after implementation.

## Task 1: Default `runs=1`

- [ ] **Step 1: Write failing test expectation**

In `tests/ktp_e2e_bench_cli.rs`, add this assertion to
`ktp_e2e_bench_cli_reports_runtime_ingress_egress_throughput` after
`clients=1`:

```rust
assert!(stdout.contains("runs=1"));
```

- [ ] **Step 2: Verify RED**

Run:

```bash
cargo test --test ktp_e2e_bench_cli ktp_e2e_bench_cli_reports_runtime_ingress_egress_throughput -- --nocapture
```

Expected: FAIL because stdout lacks `runs=1`.

- [ ] **Step 3: Minimal implementation**

In `src/bin/ktp-e2e-bench.rs`, add `runs`:

```rust
struct BenchConfig {
    runs: usize,
    clients: usize,
    frames: usize,
    payload_bytes: usize,
}
```

In `parse_args`, initialize and return it:

```rust
let mut runs = 1usize;
```

```rust
Ok(BenchConfig {
    runs,
    clients,
    frames,
    payload_bytes,
})
```

Update the single-run output string to include `runs={}` before `clients={}`.

- [ ] **Step 4: Verify GREEN**

Run:

```bash
cargo test --test ktp_e2e_bench_cli ktp_e2e_bench_cli_reports_runtime_ingress_egress_throughput -- --nocapture
```

Expected: PASS.

## Task 2: `--runs 0` Validation

- [ ] **Step 1: Write failing validation test**

Add this test to `tests/ktp_e2e_bench_cli.rs`:

```rust
#[test]
fn ktp_e2e_bench_cli_rejects_zero_runs() {
    let exe = std::env::var("CARGO_BIN_EXE_ktp-e2e-bench")
        .expect("ktp-e2e-bench binary should be built by cargo");

    let output = Command::new(exe)
        .args(["--runs", "0"])
        .output()
        .expect("ktp-e2e-bench should run");

    assert!(
        !output.status.success(),
        "ktp-e2e-bench unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--runs must be greater than zero"));
}
```

- [ ] **Step 2: Verify RED**

Run:

```bash
cargo test --test ktp_e2e_bench_cli ktp_e2e_bench_cli_rejects_zero_runs -- --nocapture
```

Expected: FAIL because `--runs` is unknown.

- [ ] **Step 3: Parse `--runs`**

Add this parse arm before `--clients`:

```rust
"--runs" => {
    runs = parse_positive_usize(next_value(&mut args, "--runs")?, "--runs")?
}
```

Update usage:

```rust
eprintln!("usage: ktp-e2e-bench [--runs N] [--clients N] [--frames N] [--payload-bytes BYTES]");
```

- [ ] **Step 4: Verify GREEN**

Run:

```bash
cargo test --test ktp_e2e_bench_cli ktp_e2e_bench_cli_rejects_zero_runs -- --nocapture
```

Expected: PASS.

## Task 3: Repeated Run Summary

- [ ] **Step 1: Write failing summary test**

Add this test to `tests/ktp_e2e_bench_cli.rs`:

```rust
#[test]
fn ktp_e2e_bench_cli_reports_repeated_run_statistics() {
    let exe = std::env::var("CARGO_BIN_EXE_ktp-e2e-bench")
        .expect("ktp-e2e-bench binary should be built by cargo");

    let output = Command::new(exe)
        .args([
            "--runs",
            "2",
            "--clients",
            "2",
            "--frames",
            "2",
            "--payload-bytes",
            "128",
        ])
        .output()
        .expect("ktp-e2e-bench should run");

    assert!(
        output.status.success(),
        "ktp-e2e-bench failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("runs=2"));
    assert!(stdout.contains("clients=2"));
    assert!(stdout.contains("bytes=512"));
    assert!(stdout.contains("elapsed_ms_min="));
    assert!(stdout.contains("elapsed_ms_median="));
    assert!(stdout.contains("elapsed_ms_max="));
    assert!(stdout.contains("throughput_mib_s_min="));
    assert!(stdout.contains("throughput_mib_s_median="));
    assert!(stdout.contains("throughput_mib_s_max="));
}
```

- [ ] **Step 2: Verify RED**

Run:

```bash
cargo test --test ktp_e2e_bench_cli ktp_e2e_bench_cli_reports_repeated_run_statistics -- --nocapture
```

Expected: FAIL because `--runs 2` still emits single-run fields.

- [ ] **Step 3: Implement sample and summary formatting**

In `src/bin/ktp-e2e-bench.rs`, add:

```rust
#[derive(Clone, Copy, Debug)]
struct BenchSample {
    elapsed_ms: f64,
    throughput_mib_s: f64,
}
```

Change `run_benchmark(config)` so it loops:

```rust
let mut samples = Vec::with_capacity(config.runs);
for _ in 0..config.runs {
    samples.push(run_benchmark_once(config)?);
}
Ok(format_report(config, &samples))
```

Move the existing benchmark body to:

```rust
fn run_benchmark_once(config: BenchConfig) -> BenchResult<BenchSample>
```

Return:

```rust
Ok(BenchSample {
    elapsed_ms: elapsed.as_secs_f64() * 1000.0,
    throughput_mib_s,
})
```

Add:

```rust
fn format_report(config: BenchConfig, samples: &[BenchSample]) -> String
fn median(sorted_values: &[f64]) -> f64
```

`format_report` should preserve single-run fields for `runs == 1` and use
summary fields for `runs > 1`.

- [ ] **Step 4: Verify GREEN**

Run:

```bash
cargo test --test ktp_e2e_bench_cli ktp_e2e_bench_cli_reports_repeated_run_statistics -- --nocapture
```

Expected: PASS.

## Task 4: Verification and Documentation

- [ ] **Step 1: Run local verification**

Run:

```bash
cargo test --test ktp_e2e_bench_cli -- --nocapture
cargo test --test tunnel_async_runtime --test tunnel_data --test tunnel_runtime --test ktp_bench_cli --test ktp_e2e_bench_cli
cargo check --bins
cargo fmt --check
git diff --check
```

Expected: all commands exit 0.

- [ ] **Step 2: Commit and push implementation**

Run:

```bash
git add src/bin/ktp-e2e-bench.rs tests/ktp_e2e_bench_cli.rs
git commit -m "test: add ktp e2e repeated run statistics"
git push origin main
```

Expected: GitHub main contains the implementation commit.

- [ ] **Step 3: Run Linux release evidence**

Run from Windows:

```powershell
ssh -i $env:USERPROFILE\.ssh\codex_keli_ed25519 -o BatchMode=yes root@2.56.116.39 "cd /root/kelicloud-agent-rs-bench && git fetch origin && git reset --hard origin/main && cargo build --release --bin ktp-e2e-bench && ./target/release/ktp-e2e-bench --runs 3 --clients 1 --frames 1024 --payload-bytes 1024 && ./target/release/ktp-e2e-bench --runs 3 --clients 4 --frames 256 --payload-bytes 1024 && ./target/release/ktp-e2e-bench --runs 3 --clients 4 --frames 256 --payload-bytes 16384"
```

Expected: all commands print repeated-run min/median/max fields.

- [ ] **Step 4: Update docs**

Modify `docs/ktp-benchmarks.md` with a repeated-run table and commit:

```bash
git add docs/ktp-benchmarks.md
git commit -m "docs: record ktp e2e repeated run benchmark"
git push origin main
```

Expected: GitHub main contains the documentation commit.

## Self-Review

- Spec coverage: The plan covers default `runs=1`, validation, repeated summary
  fields, local verification, Linux evidence, and docs.
- Placeholder scan: No placeholder markers or vague implementation steps remain.
- Type consistency: `runs`, `BenchSample`, `elapsed_ms_*`, and
  `throughput_mib_s_*` are named consistently across tests and implementation.
