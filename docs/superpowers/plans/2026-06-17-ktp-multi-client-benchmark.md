# KTP Multi-Client Benchmark Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `--clients N` to `ktp-e2e-bench` so Linux release benchmarks can measure aggregate KTP runtime throughput across multiple simultaneous TCP sessions.

**Architecture:** Keep this phase inside benchmark tooling and documentation. `ktp-e2e-bench` will spawn multiple local TCP clients and multiple echo-target handlers, then use one shared ingress runtime and one shared egress runtime to relay all KTP frame types until the aggregate response byte count is observed. Production tunnel runtime behavior remains unchanged.

**Tech Stack:** Rust 2021, standard library TCP/threading, existing `TunnelTcpRuntime`, existing KTP frame types, existing Cargo integration tests.

---

## File Structure

- Modify `tests/ktp_e2e_bench_cli.rs`
  - Owns CLI behavior tests for the benchmark binary.
  - Adds default `clients=1`, explicit `--clients 2`, and invalid `--clients 0` coverage.
- Modify `src/bin/ktp-e2e-bench.rs`
  - Adds `clients` to `BenchConfig`.
  - Parses `--clients`.
  - Spawns multiple client threads.
  - Accepts multiple echo target sessions.
  - Replaces open-then-data relay with a generic bidirectional relay loop.
- Modify `docs/ktp-benchmarks.md`
  - Records Linux release evidence after the implementation is verified.

No production files should change in this phase.

## Task 1: CLI Default Output Test

**Files:**
- Modify: `tests/ktp_e2e_bench_cli.rs`

- [ ] **Step 1: Write the failing test expectation**

Add this assertion to `ktp_e2e_bench_cli_reports_runtime_ingress_egress_throughput` after the `bridge=batch` assertion:

```rust
assert!(stdout.contains("clients=1"));
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --test ktp_e2e_bench_cli -- --nocapture
```

Expected: FAIL because stdout does not contain `clients=1`.

- [ ] **Step 3: Add minimal output support**

Modify `src/bin/ktp-e2e-bench.rs`:

```rust
struct BenchConfig {
    clients: usize,
    frames: usize,
    payload_bytes: usize,
}
```

In `parse_args`, initialize:

```rust
let mut clients = 1usize;
```

Return:

```rust
Ok(BenchConfig {
    clients,
    frames,
    payload_bytes,
})
```

Update output format to include `clients`:

```rust
"ktp_e2e_bench mode=runtime_ingress_egress transport=ktp_tcp bridge=batch clients={} frames={} payload_bytes={} bytes={} elapsed_ms={:.3} throughput_mib_s={:.3}",
config.clients,
config.frames,
config.payload_bytes,
bytes,
elapsed.as_secs_f64() * 1000.0,
throughput_mib_s
```

Keep `bytes` unchanged in this task so the default test passes without changing runtime behavior.

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test --test ktp_e2e_bench_cli -- --nocapture
```

Expected: PASS.

## Task 2: `--clients` Argument Validation

**Files:**
- Modify: `tests/ktp_e2e_bench_cli.rs`
- Modify: `src/bin/ktp-e2e-bench.rs`

- [ ] **Step 1: Write invalid clients test**

Add this test:

```rust
#[test]
fn ktp_e2e_bench_cli_rejects_zero_clients() {
    let exe = std::env::var("CARGO_BIN_EXE_ktp-e2e-bench")
        .expect("ktp-e2e-bench binary should be built by cargo");

    let output = Command::new(exe)
        .args(["--clients", "0"])
        .output()
        .expect("ktp-e2e-bench should run");

    assert!(
        !output.status.success(),
        "ktp-e2e-bench unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--clients must be greater than zero"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --test ktp_e2e_bench_cli ktp_e2e_bench_cli_rejects_zero_clients -- --nocapture
```

Expected: FAIL because `--clients` is an unknown argument.

- [ ] **Step 3: Parse `--clients`**

In `parse_args`, add this match arm before `--frames`:

```rust
"--clients" => {
    clients = parse_positive_usize(next_value(&mut args, "--clients")?, "--clients")?
}
```

Update usage:

```rust
eprintln!("usage: ktp-e2e-bench [--clients N] [--frames N] [--payload-bytes BYTES]");
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test --test ktp_e2e_bench_cli ktp_e2e_bench_cli_rejects_zero_clients -- --nocapture
```

Expected: PASS.

## Task 3: Multi-Client CLI Behavior Test

**Files:**
- Modify: `tests/ktp_e2e_bench_cli.rs`

- [ ] **Step 1: Write explicit multi-client test**

Add this test:

```rust
#[test]
fn ktp_e2e_bench_cli_reports_multi_client_aggregate_throughput() {
    let exe = std::env::var("CARGO_BIN_EXE_ktp-e2e-bench")
        .expect("ktp-e2e-bench binary should be built by cargo");

    let output = Command::new(exe)
        .args(["--clients", "2", "--frames", "2", "--payload-bytes", "128"])
        .output()
        .expect("ktp-e2e-bench should run");

    assert!(
        output.status.success(),
        "ktp-e2e-bench failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("clients=2"));
    assert!(stdout.contains("frames=2"));
    assert!(stdout.contains("payload_bytes=128"));
    assert!(stdout.contains("bytes=512"));
    assert!(stdout.contains("throughput_mib_s="));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --test ktp_e2e_bench_cli ktp_e2e_bench_cli_reports_multi_client_aggregate_throughput -- --nocapture
```

Expected: FAIL because the binary does not yet run two clients or compute aggregate bytes.

## Task 4: Implement Multi-Client Benchmark Runtime

**Files:**
- Modify: `src/bin/ktp-e2e-bench.rs`

- [ ] **Step 1: Compute aggregate bytes**

Replace:

```rust
let bytes = config.frames * config.payload_bytes;
```

with:

```rust
let bytes = config.clients * config.frames * config.payload_bytes;
```

- [ ] **Step 2: Accept multiple echo target sessions**

Replace the existing `echo_thread` body with:

```rust
let clients = config.clients;
let echo_thread = thread::spawn(move || -> std::io::Result<()> {
    let mut handles = Vec::with_capacity(clients);
    for _ in 0..clients {
        let (mut stream, _) = target.accept()?;
        let handle = thread::spawn(move || -> std::io::Result<()> {
            let mut buffer = vec![0u8; payload_bytes];
            for _ in 0..frames {
                stream.read_exact(&mut buffer)?;
                stream.write_all(&buffer)?;
            }
            Ok(())
        });
        handles.push(handle);
    }
    for handle in handles {
        handle
            .join()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "echo thread panicked"))??;
    }
    Ok(())
});
```

- [ ] **Step 3: Spawn multiple client sessions**

Replace the existing single `client_thread` with:

```rust
let client_threads = (0..config.clients)
    .map(|_| {
        thread::spawn(move || -> std::io::Result<()> {
            let mut stream = connect_with_retry(("127.0.0.1", listen_port))?;
            let payload = vec![0x5a; payload_bytes];
            let mut response = vec![0u8; payload_bytes];
            for _ in 0..frames {
                stream.write_all(&payload)?;
                stream.read_exact(&mut response)?;
            }
            Ok(())
        })
    })
    .collect::<Vec<_>>();
```

This closure uses only `Copy` values, so each spawned thread receives its own
copy of `listen_port`, `payload_bytes`, and `frames`.

- [ ] **Step 4: Join all client sessions**

Replace the single client join with:

```rust
for client_thread in client_threads {
    client_thread
        .join()
        .map_err(|_| "client thread panicked")??;
}
```

- [ ] **Step 5: Replace open-specific relay with generic frame relay**

Delete this call:

```rust
relay_open(&mut ingress_runtime, &mut egress_runtime)?;
```

Keep:

```rust
relay_data_batches(&mut ingress_runtime, &mut egress_runtime, bytes)?;
```

Remove the `relay_open` function after the generic relay is implemented.

- [ ] **Step 6: Forward immediate responses inside `relay_data_batches`**

Replace the ingress drain body:

```rust
for frame in ingress_runtime.next_client_frames(RELAY_BATCH_FRAMES)? {
    if frame.frame_type == FrameType::SessionData {
        ingress_bytes += frame.payload.len();
    }
    egress_runtime.handle_server_frame(to_leg(frame, FrameLeg::Egress))?;
}
```

with:

```rust
for frame in ingress_runtime.next_client_frames(RELAY_BATCH_FRAMES)? {
    if frame.frame_type == FrameType::SessionData {
        ingress_bytes += frame.payload.len();
    }
    let responses = egress_runtime.handle_server_frame(to_leg(frame, FrameLeg::Egress))?;
    for response in responses {
        ingress_runtime.handle_server_frame(to_leg(response, FrameLeg::Ingress))?;
    }
}
```

Replace the egress drain body:

```rust
for frame in egress_runtime.next_client_frames(RELAY_BATCH_FRAMES)? {
    if frame.frame_type == FrameType::SessionData {
        egress_bytes += frame.payload.len();
    }
    ingress_runtime.handle_server_frame(to_leg(frame, FrameLeg::Ingress))?;
}
```

with:

```rust
for frame in egress_runtime.next_client_frames(RELAY_BATCH_FRAMES)? {
    if frame.frame_type == FrameType::SessionData {
        egress_bytes += frame.payload.len();
    }
    let responses = ingress_runtime.handle_server_frame(to_leg(frame, FrameLeg::Ingress))?;
    for response in responses {
        egress_runtime.handle_server_frame(to_leg(response, FrameLeg::Egress))?;
    }
}
```

This prevents session data from being dropped when `SESSION_DATA` frames arrive
between multiple `SESSION_OPEN` frames.

- [ ] **Step 7: Run multi-client test to verify it passes**

Run:

```bash
cargo test --test ktp_e2e_bench_cli ktp_e2e_bench_cli_reports_multi_client_aggregate_throughput -- --nocapture
```

Expected: PASS.

## Task 5: Regression Test Set

**Files:**
- No new files.

- [ ] **Step 1: Run benchmark CLI tests**

Run:

```bash
cargo test --test ktp_e2e_bench_cli -- --nocapture
```

Expected: all tests PASS.

- [ ] **Step 2: Run tunnel regression tests**

Run:

```bash
cargo test --test tunnel_async_runtime --test tunnel_data --test tunnel_runtime --test ktp_bench_cli --test ktp_e2e_bench_cli
```

Expected: all tests PASS.

- [ ] **Step 3: Run build and formatting checks**

Run:

```bash
cargo check --bins
cargo fmt --check
git diff --check
```

Expected: all commands exit 0.

## Task 6: Linux Benchmark Evidence

**Files:**
- Modify: `docs/ktp-benchmarks.md`

- [ ] **Step 1: Push implementation branch or commit to GitHub**

Run:

```bash
git status --short
git log --oneline -3
git push origin main
```

Expected: GitHub main contains the implementation commit.

- [ ] **Step 2: Pull on Linux benchmark host**

Run from Windows:

```powershell
ssh -i $env:USERPROFILE\.ssh\codex_keli_ed25519 -o BatchMode=yes root@2.56.116.39 "cd /root/kelicloud-agent-rs-bench && git fetch origin && git reset --hard origin/main && git rev-parse --short HEAD"
```

Expected: output commit matches local `git rev-parse --short HEAD`.

- [ ] **Step 3: Build and run Linux release benchmarks**

Run from Windows:

```powershell
ssh -i $env:USERPROFILE\.ssh\codex_keli_ed25519 -o BatchMode=yes root@2.56.116.39 "cd /root/kelicloud-agent-rs-bench && cargo build --release --bin ktp-e2e-bench && ./target/release/ktp-e2e-bench --clients 1 --frames 1024 --payload-bytes 1024 && ./target/release/ktp-e2e-bench --clients 4 --frames 256 --payload-bytes 1024 && ./target/release/ktp-e2e-bench --clients 4 --frames 256 --payload-bytes 16384"
```

Expected: all benchmark commands exit 0 and print `clients=...`, `bytes=...`,
`elapsed_ms=...`, and `throughput_mib_s=...`.

- [ ] **Step 4: Update benchmark documentation**

Add a multi-client subsection to `docs/ktp-benchmarks.md` with the commit hash
and measured rows:

```markdown
Runtime ingress-to-egress multi-client path:

| Clients | Frames / Client | Payload | Bytes | Elapsed | Throughput |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 1024 | 1024 B | ... | ... ms | ... MiB/s |
| 4 | 256 | 1024 B | ... | ... ms | ... MiB/s |
| 4 | 256 | 16384 B | ... | ... ms | ... MiB/s |
```

- [ ] **Step 5: Verify docs-only update**

Run:

```bash
git diff --check
cargo test --test ktp_e2e_bench_cli -- --nocapture
```

Expected: both commands exit 0.

- [ ] **Step 6: Commit and push benchmark documentation**

Run:

```bash
git add docs/ktp-benchmarks.md
git commit -m "docs: record ktp multi-client benchmark"
git push origin main
```

Expected: GitHub main contains the benchmark evidence commit.

## Self-Review

- Spec coverage: The plan covers `--clients`, default compatibility, invalid
  argument handling, multiple clients, multiple echo sessions, aggregate bytes,
  Linux evidence, docs update, and rollback through benchmark-only changes.
- Placeholder scan: No placeholder markers or vague implementation steps are left.
- Type consistency: `BenchConfig.clients`, `clients=<N>`, and aggregate `bytes`
  are used consistently across tests, implementation, and docs.
