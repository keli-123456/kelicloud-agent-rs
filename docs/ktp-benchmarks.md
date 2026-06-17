# KTP Benchmark Notes

This file records repeatable KTP data-plane benchmark evidence. Treat these
numbers as engineering baselines, not production capacity promises.

## 2026-06-17 Linux Release Baseline

Code:

- Repository: `kelicloud-agent-rs`
- Commit: `9fe3b83` for carrier results
- Commit: `fcf21a8` for end-to-end runtime results with batched frame drain
- Commit: `12fc85c` for multi-client end-to-end runtime results
- Commit: `a06916c` for repeated-run end-to-end runtime statistics
- Commit: `c86a832` for relay-loop diagnostic counters
- Commit: `d1355da` for condition-wait relay prototype
- Commit: `ca46474` for shared relay readiness notification
- Commit: `23551b8` for production tunnel-data loop readiness scheduling
- Commit: `56b562f` for local production tunnel-data diagnostics counters
- Commit: `7760cd3` for production runtime-wait latency percentile diagnostics
- Commit: `9590a4f` for production outbound queue dwell diagnostics
- Carrier binary: `ktp-tunnel-bench`
- End-to-end binary: `ktp-e2e-bench`
- Build mode: `cargo build --release --bin <bench>`

Host:

- OS: Debian GNU/Linux 12
- Kernel: `6.1.0-31-amd64`
- Architecture: `x86_64`
- Memory: about 3.8 GiB

Command shape:

```bash
./target/release/ktp-tunnel-bench --frames <N> --payload-bytes <BYTES> --runs 3
```

Results:

Encrypted carrier only:

| Frames | Payload | Total Bytes | Elapsed | Throughput |
| ---: | ---: | ---: | ---: | ---: |
| 65536 | 1024 B | 201326592 | 1258.015 ms | 152.621 MiB/s |
| 4096 | 16384 B | 201326592 | 647.507 ms | 296.522 MiB/s |
| 2048 | 65536 B | 402653184 | 1084.613 ms | 354.043 MiB/s |

Runtime ingress-to-egress path:

```bash
./target/release/ktp-e2e-bench --frames <N> --payload-bytes <BYTES>
```

| Frames | Payload | Bytes | Elapsed | Throughput |
| ---: | ---: | ---: | ---: | ---: |
| 1024 | 1024 B | 1048576 | 219.557 ms | 4.555 MiB/s |
| 256 | 16384 B | 4194304 | 113.265 ms | 35.315 MiB/s |

Runtime ingress-to-egress multi-client path:

```bash
./target/release/ktp-e2e-bench --clients <N> --frames <N> --payload-bytes <BYTES>
```

| Clients | Frames / Client | Payload | Bytes | Elapsed | Throughput |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 1024 | 1024 B | 1048576 | 370.949 ms | 2.696 MiB/s |
| 4 | 256 | 1024 B | 1048576 | 159.376 ms | 6.274 MiB/s |
| 4 | 256 | 16384 B | 16777216 | 175.639 ms | 91.096 MiB/s |

Runtime ingress-to-egress repeated-run path:

```bash
./target/release/ktp-e2e-bench --runs 3 --clients <N> --frames <N> --payload-bytes <BYTES>
```

| Runs | Clients | Frames / Client | Payload | Bytes / Run | Elapsed Min | Elapsed Median | Elapsed Max | Throughput Min | Throughput Median | Throughput Max |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3 | 1 | 1024 | 1024 B | 1048576 | 366.064 ms | 455.854 ms | 476.726 ms | 2.098 MiB/s | 2.194 MiB/s | 2.732 MiB/s |
| 3 | 4 | 256 | 1024 B | 1048576 | 270.165 ms | 298.625 ms | 301.182 ms | 3.320 MiB/s | 3.349 MiB/s | 3.701 MiB/s |
| 3 | 4 | 256 | 16384 B | 16777216 | 212.048 ms | 230.662 ms | 275.564 ms | 58.063 MiB/s | 69.366 MiB/s | 75.454 MiB/s |

Runtime relay-loop diagnostics:

```bash
./target/release/ktp-e2e-bench --diagnostics --runs 3 --clients <N> --frames <N> --payload-bytes <BYTES>
```

| Runs | Clients | Payload | Relay Turns | Empty Turns | Yield Turns | Ingress Frames | Egress Frames | Ingress Data Frames | Egress Data Frames |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3 | 1 | 1024 B | 376226 | 370089 | 376223 | 3075 | 3075 | 3072 | 3072 |
| 3 | 4 | 1024 B | 483878 | 478957 | 483875 | 3093 | 3082 | 3072 | 3072 |
| 3 | 4 | 16384 B | 403808 | 399467 | 403805 | 3092 | 3083 | 3072 | 3072 |

Runtime relay-loop condition-wait prototype:

```bash
./target/release/ktp-e2e-bench --diagnostics --relay-wait-timeout-us 10 --runs 3 --clients <N> --frames <N> --payload-bytes <BYTES>
```

| Runs | Clients | Payload | Elapsed Median | Throughput Median | Relay Turns | Empty Turns | Yield Turns | Wait Turns | Ingress Frames | Egress Frames |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3 | 1 | 1024 B | 345.446 ms | 2.895 MiB/s | 8484 | 2338 | 8481 | 7279 | 3075 | 3074 |
| 3 | 4 | 1024 B | 186.541 ms | 5.361 MiB/s | 5045 | 807 | 5042 | 3485 | 3093 | 3084 |
| 3 | 4 | 16384 B | 217.487 ms | 73.568 MiB/s | 5065 | 1348 | 5062 | 3311 | 3093 | 3084 |

Runtime relay-loop shared-readiness wait:

```bash
./target/release/ktp-e2e-bench --diagnostics --relay-wait-timeout-us 100 --runs 3 --clients <N> --frames <N> --payload-bytes <BYTES>
```

| Runs | Clients | Payload | Elapsed Median | Throughput Median | Relay Turns | Empty Turns | Yield Turns | Wait Turns | Ingress Frames | Egress Frames |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3 | 1 | 1024 B | 368.920 ms | 2.711 MiB/s | 8017 | 1873 | 8014 | 7913 | 3075 | 3074 |
| 3 | 4 | 1024 B | 217.181 ms | 4.604 MiB/s | 4382 | 982 | 4379 | 3271 | 3093 | 3083 |
| 3 | 4 | 16384 B | 187.193 ms | 85.473 MiB/s | 5062 | 933 | 5059 | 3850 | 3093 | 3084 |

Production data-loop readiness integration check:

```bash
cargo build --release --bin kelicloud-agent-rs --bin ktp-e2e-bench
./target/release/ktp-e2e-bench --diagnostics --relay-wait-timeout-us 100 --runs 3 --clients 4 --frames 256 --payload-bytes 1024
```

| Commit | Runs | Clients | Payload | Elapsed Median | Throughput Median | Relay Turns | Empty Turns | Wait Turns |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `23551b8` | 3 | 4 | 1024 B | 176.353 ms | 5.670 MiB/s | 4092 | 654 | 2833 |
| `56b562f` | 3 | 4 | 1024 B | 219.319 ms | 4.560 MiB/s | 4273 | 954 | 3214 |
| `7760cd3` | 3 | 4 | 1024 B | 204.894 ms | 4.881 MiB/s | 4277 | 882 | 3137 |
| `9590a4f` | 3 | 4 | 1024 B | 162.743 ms | 6.145 MiB/s | 4268 | 619 | 3010 |

Runtime small-frame latency evidence:

```bash
./target/release/ktp-e2e-bench --latency --clients <N> --frames <N> --payload-bytes <BYTES>
```

The `--latency` flag records each client echo round trip inside the benchmark
client threads and appends `rtt_micros_samples`, `rtt_micros_p50`,
`rtt_micros_p95`, `rtt_micros_p99`, and `rtt_micros_max` to the report. Keep it
off for raw throughput-only samples; turn it on when comparing small-frame
interactive paths such as RDP-like forwarding.

Runtime RDP-like mixed-payload evidence:

```bash
./target/release/ktp-e2e-bench --profile rdp-like --diagnostics --latency --relay-wait-timeout-us 100 --runs 3 --clients <N> --frames <N> --payload-bytes <MAX_BYTES>
```

The RDP-like profile keeps the same ingress-to-egress runtime path but sends a
deterministic mix of small interactive frames and occasional larger refresh
bursts. In this mode, `--payload-bytes` is the cap for any one benchmark frame,
and the report's `bytes` field is the aggregate mixed-payload byte count.

Live KTP tunnel diagnostics evidence:

```bash
scripts/ktp-live-canary-evidence.sh \
  --service-name kelicloud-agent-rs \
  --since "30 minutes ago" \
  --evidence-file ktp-live-canary.evidence.md
```

During a real KTP canary window, run the helper after sending tunnel traffic.
It reads `journalctl` by default, or `--log-file <path>` for captured agent
logs, and verifies that `tunnel data diagnostics` lines include runtime wait
and outbound queue dwell percentile fields. Treat the generated Markdown file
as the live-log companion to `ktp-e2e-bench --latency` output.

Local backend KTP smoke:

```bash
KELICLOUD_SMOKE_KTP_TCP=true scripts/smoke-local-backend.sh
```

This runs the regular local real-backend smoke with the backend KTP TCP relay
enabled and the agent started with `--tunnel-ktp-tcp-address`. After the tunnel
echo check succeeds, the script waits for live `tunnel data diagnostics` in the
captured agent log and writes `smoke-logs/ktp-live-canary.evidence.md`.

## 2026-06-18 Release Host Latency Sample

Code:

- Repository: `kelicloud-agent-rs`
- Commit: `c4cd594`
- Carrier binary: `ktp-tunnel-bench`
- End-to-end binary: `ktp-e2e-bench`
- Build mode: `cargo build --release --bin ktp-tunnel-bench --bin ktp-e2e-bench`

Host:

- OS: Debian GNU/Linux 12 (bookworm)
- Kernel: `6.1.0-31-amd64`
- Architecture: `x86_64`
- CPU: Intel(R) Xeon(R) CPU E5-2690 v4 @ 2.60GHz
- CPU cores: 4
- Memory: 3.8 GiB

Commands:

```bash
./target/release/ktp-tunnel-bench --runs 3 --frames 4096 --payload-bytes 16384
./target/release/ktp-e2e-bench --diagnostics --latency --relay-wait-timeout-us 100 --runs 3 --clients 4 --frames 64 --payload-bytes 1024
```

Encrypted carrier only:

| Runs | Frames | Payload | Total Bytes | Elapsed | Throughput |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 3 | 4096 | 16384 B | 201326592 | 647.616 ms | 296.472 MiB/s |

Runtime ingress-to-egress small-frame latency:

| Runs | Clients | Frames / Client | Payload | Bytes / Run | Elapsed Median | Throughput Median | RTT p50 | RTT p95 | RTT p99 | RTT Max |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3 | 4 | 64 | 1024 B | 262144 | 59.555 ms | 4.198 MiB/s | 433 us | 2444 us | 4797 us | 9448 us |

Runtime relay-loop diagnostics:

| Runs | Clients | Payload | Relay Turns | Empty Turns | Yield Turns | Wait Turns | Ingress Frames | Egress Frames | Ingress Data Frames | Egress Data Frames |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3 | 4 | 1024 B | 1257 | 255 | 1254 | 953 | 789 | 779 | 768 | 768 |

Observations:

- Small 1 KiB frames are dominated by per-frame overhead.
- 16 KiB and 64 KiB payloads show much higher encrypted TCP carrier throughput.
- Multi-client e2e measurement now has explicit aggregate bytes and client
  counts. On this host, four concurrent 1 KiB clients improved aggregate
  throughput over the one-client sample, which suggests relay scheduling should
  be evaluated with concurrent sessions instead of single-client numbers only.
- End-to-end runtime throughput is still far below carrier-only throughput, so
  the next bottleneck is runtime relay scheduling and per-frame session
  handling, not ChaCha20-Poly1305 encryption itself.
- Batched runtime frame drain keeps the large-payload path around the previous
  baseline and improves the documented 1 KiB baseline, but it is only the first
  step. Future optimization should focus on read/write scheduling, multi-session
  relay fairness, and reducing ingress/egress relay round trips before changing
  cryptography.
- Run-to-run variance is visible on this small Linux host, so later performance
  gates should use repeated runs or percentile summaries instead of one-off
  wall-clock samples.
- `--runs 3` confirms that the 4-client 16 KiB path still has much better
  aggregate throughput than small-frame tests, but variance is large enough that
  future runtime changes should compare median and min/max spread, not just the
  fastest sample.
- Relay diagnostics show hundreds of thousands of relay-loop turns for roughly
  three thousand data frames. More than 98% of turns are empty and nearly every
  turn calls `thread::yield_now()`. The next optimization should replace this
  bench/runtime polling shape with condition-based waiting or readiness
  notification before changing cryptography or frame encoding.
- A 10 microsecond condition-wait prototype cuts relay turns from hundreds of
  thousands to roughly five to eight thousand over the same three-run samples.
  This validates queue notification as the right optimization direction.
- A 100 microsecond wait was too aggressive in one 4-client small-frame
  repeated run and timed out before all bytes relayed. The production design
  should avoid blind long waits and prefer readiness notification across both
  ingress and egress queues.
- Shared relay readiness replaces that blind one-sided wait in the benchmark:
  both ingress and egress runtimes attach to one notifier, and the relay loop
  waits only after both queues are empty. On the same Linux host, the 100
  microsecond 4-client small-frame repeated run completed without the earlier
  timeout and kept relay turns near the 10 microsecond prototype range.
- Production KTP TCP tunnel-data sessions now opt into a short runtime-frame
  wait before idle socket reads and use a short idle socket read timeout. The
  default websocket path keeps the older blocking behavior because the runtime
  scheduling hints default to disabled.
- Production tunnel-data sessions now have local diagnostics counters for
  runtime wait attempts, wait hits, wait elapsed microseconds, outbound runtime
  frames, socket idle reads, and empty idle reads. The counters are not sent in
  the KTP protocol; the agent logs a sanitized summary after KTP data-session
  reconnect boundaries when there is activity.
- Runtime wait elapsed diagnostics now include p50, p95, and p99 microsecond
  bucket upper bounds. This keeps production logs cheap while making wait-time
  regressions visible without exporting raw per-frame samples.
- Production KTP runtime queues now timestamp outbound frames at enqueue and
  record queue dwell at dequeue. Tunnel-data diagnostics expose cumulative
  frames, total/max microseconds, and p50/p95/p99 bucket upper bounds, so logs
  can distinguish scheduler wait from time spent sitting in the runtime queue.
- The `9590a4f` sample did not show an obvious benchmark regression from the
  enqueue timestamp and dequeue bucket accounting, but the run-to-run variance
  above still means this should be treated as a measurement point, not proof of
  a performance improvement.
- `ktp-e2e-bench --latency` now captures client-observed round-trip percentiles
  for small-frame paths. This gives us a lightweight local signal for
  interactivity before moving to live canary traffic.
- The `c4cd594` release-host sample records the same encrypted carrier
  throughput shape as the earlier baseline, and adds small-frame RTT percentiles
  from the runtime ingress-to-egress path. The p95/p99 gap shows why live canary
  traffic should capture latency and runtime queue diagnostics together.

## 2026-06-18 KTP Local Backend Smoke

Code:

- Repository: `kelicloud-agent-rs`
- Commit: `bb06992`
- Backend ref: `keli-123456/kelicloud@main`
- Frontend ref: `keli-123456/kelicloud-web@main`

Host:

- OS: Debian GNU/Linux 12
- Kernel: `6.1.0-31-amd64`
- Architecture: `x86_64`
- Go: `go1.24.11`
- Node: `v22.22.3`

Command shape:

```bash
KELICLOUD_SMOKE_KTP_TCP=true \
KOMARI_DB_USER=komari_smoke \
KOMARI_DB_PASS=komari-smoke-pass \
KOMARI_DB_NAME=komari_ktp-smoke-20260617134406 \
BACKEND_LISTEN=127.0.0.1:26776 \
BACKEND_ENDPOINT=http://127.0.0.1:26776 \
scripts/smoke-local-backend.sh
```

Result:

- Backend KTP TCP relay listened on `127.0.0.1:40699`.
- Tunnel rule echo succeeded through `127.0.0.1:51529`.
- `smoke-summary --require-pass` passed startup, basic info, report WebSocket,
  report send, ping upload, exec upload, terminal, and CN connectivity checks.
- KTP evidence was generated at
  `/tmp/kelicloud-agent-rs-ktp-smoke-20260617134406/smoke-logs/ktp-live-canary.evidence.md`.

Latest observed diagnostics:

```text
tunnel data diagnostics: runtime_wait_attempts=9350 runtime_wait_hits=1 runtime_wait_elapsed_micros_total=5649352 runtime_wait_elapsed_micros_max=19949 runtime_wait_elapsed_p50_micros=250 runtime_wait_elapsed_p95_micros=5000 runtime_wait_elapsed_p99_micros=10000 outbound_runtime_frames=5 outbound_queue_dwell_frames=5 outbound_queue_dwell_micros_total=11385 outbound_queue_dwell_micros_max=5494 outbound_queue_dwell_p50_micros=250 outbound_queue_dwell_p95_micros=10000 outbound_queue_dwell_p99_micros=10000 socket_idle_reads=9350 socket_idle_empty_reads=9345
```

Notes:

- The first real KTP local-backend smoke on `c61617c` reached rule creation but
  failed with `relay_unavailable`. Agent logs showed a panic at
  `src/tunnel_data.rs:568`: `there is no reactor running`. The `bb06992` fix
  moved the KTP idle `tokio::time::timeout` creation inside the socket's Tokio
  runtime and the same smoke then passed.
- The successful smoke proves the current KTP TCP carrier can run the existing
  control-plane compatibility suite and a loopback tunnel echo against a real
  backend. It is not yet a high-throughput production capacity claim.
- The `Local Backend Smoke` GitHub Actions workflow now runs the same smoke as
  a matrix over `websocket` and `ktp_tcp`, with separate artifacts for each data
  carrier. That moves KTP smoke evidence from one-off host logs into CI
  artifacts on pushes to `main`.

Next evidence to collect:

- Repeated multi-client runs with higher sample counts and percentile summaries.
- Release-host samples for `ktp-e2e-bench --profile rdp-like` so tunnel tuning
  compares interactive mixed-payload traffic instead of fixed-size frames only.
- Before/after diagnostics for production data carrier scheduling changes.
- Inspect the next GitHub Actions KTP local-backend artifact and keep it as the
  release evidence source instead of relying on one-off remote host paths.
- Live KTP canary traffic with real RDP-like forwarding and paired RTT or
  throughput evidence from the same observation window.
