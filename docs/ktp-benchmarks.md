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
and the report's `bytes` field is the aggregate mixed-payload byte count. The
runtime e2e client preallocates one max-size payload buffer per client and
reports `client_payload_reused=1`, so throughput samples are not dominated by
per-frame payload allocation.

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

## 2026-06-18 RDP-Like Profile Sample

Code:

- Repository: `kelicloud-agent-rs`
- Commit: `016dc13`
- End-to-end binary: `ktp-e2e-bench`
- Build mode: `cargo build --release --bin ktp-e2e-bench`

Host:

- OS: Debian GNU/Linux 12 (bookworm)
- Kernel: `6.1.0-31-amd64`
- Architecture: `x86_64`
- CPU: Intel(R) Xeon(R) CPU E5-2690 v4 @ 2.60GHz
- CPU cores: 4
- Memory: 3.8 GiB

Command:

```bash
./target/release/ktp-e2e-bench --profile rdp-like --diagnostics --latency --relay-wait-timeout-us 100 --runs 3 --clients 4 --frames 64 --payload-bytes 8192
```

Runtime ingress-to-egress RDP-like latency:

| Runs | Clients | Frames / Client | Max Payload | Bytes / Run | Elapsed Median | Throughput Median | RTT p50 | RTT p95 | RTT p99 | RTT Max |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3 | 4 | 64 | 8192 B | 247296 | 64.507 ms | 3.656 MiB/s | 465 us | 4186 us | 8038 us | 14440 us |

Runtime relay-loop diagnostics:

| Runs | Clients | Relay Turns | Empty Turns | Yield Turns | Wait Turns | Ingress Frames | Egress Frames | Ingress Data Frames | Egress Data Frames |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3 | 4 | 1378 | 384 | 1375 | 1028 | 789 | 779 | 768 | 768 |

Notes:

- The RDP-like profile is deterministic, so future runs can compare the same
  small-frame and burst-frame sequence instead of sampling random payloads.
- The reported `bytes` value is lower than `frames * payload_bytes` because
  `payload_bytes` caps the largest burst while most frames stay small.
- This is a first release-host evidence point for interactive mixed-payload
  traffic; it should be expanded with longer live forwarding windows before
  setting performance gates.

## 2026-06-18 Relay Batch Matrix Baseline

Code:

- Repository: `kelicloud-agent-rs`
- Commit: `9c4ff73`
- End-to-end binary: `ktp-e2e-bench`
- Build mode: release builds created by `scripts/ktp-relay-batch-matrix.sh`

Host:

- OS: Debian GNU/Linux 12 (bookworm)
- Kernel: `6.1.0-31-amd64`
- Architecture: `x86_64`
- CPU: Intel(R) Xeon(R) CPU E5-2690 v4 @ 2.60GHz
- CPU cores: 4
- Memory: 3.8 GiB

Command:

```bash
KTP_BATCH_MATRIX_CSV=/tmp/ktp-batch-matrix-default.csv \
  bash scripts/ktp-relay-batch-matrix.sh
```

Workload:

- Profile: `rdp-like`
- Runs: 3
- Clients: 2
- Frames per client: 64
- Max payload: 8192 B
- Relay wait timeout: 100 us
- Relay batch sweep: 1, 2, 4, 8, 16, 32, 64

Relay batch sweep:

| Relay Batch Frames | Elapsed Median | Throughput Median | RTT p50 | RTT p95 | RTT p99 | RTT Max | Relay Turns | Wait Turns | Max Batch In/Eg |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 23.160 ms | 5.092 MiB/s | 254 us | 915 us | 2201 us | 3015 us | 864 | 640 | 1 / 1 |
| 2 | 33.687 ms | 3.500 MiB/s | 359 us | 1254 us | 2606 us | 3764 us | 808 | 746 | 2 / 2 |
| 4 | 34.425 ms | 3.425 MiB/s | 201 us | 1313 us | 5316 us | 9489 us | 926 | 862 | 3 / 3 |
| 8 | 26.386 ms | 4.469 MiB/s | 233 us | 1481 us | 2478 us | 3293 us | 785 | 721 | 3 / 2 |
| 16 | 29.393 ms | 4.012 MiB/s | 214 us | 1581 us | 5135 us | 6984 us | 809 | 753 | 3 / 2 |
| 32 | 21.824 ms | 5.403 MiB/s | 199 us | 761 us | 2726 us | 2866 us | 731 | 619 | 3 / 2 |
| 64 | 24.272 ms | 4.858 MiB/s | 216 us | 876 us | 1930 us | 4348 us | 730 | 668 | 3 / 2 |

Notes:

- Batch 32 was the best sample for median elapsed time, median throughput, p95
  RTT, and max RTT in this run.
- Batch 64 had the best p99 RTT, but lower median throughput than batch 32.
- The observed maximum runtime batch size was still only 2 to 3 frames for the
  larger caps, so the cap is not the only bottleneck. Current RDP-like traffic
  does not naturally fill large batches on this short benchmark.
- Keep the runtime default unchanged until a longer matrix covers more clients,
  more frames, and live forwarding traffic. This result makes batch 32 a
  candidate for the next tuning pass, not a production default by itself.

## 2026-06-18 Multi-Client Relay Batch Matrix

Code:

- Repository: `kelicloud-agent-rs`
- Commit: `f960a42`
- End-to-end binary: `ktp-e2e-bench`
- Build mode: release builds created by `scripts/ktp-relay-batch-matrix.sh`
- Run directory: `/root/kelicloud-agent-rs-matrix-f960a42-a`

Host:

- OS: Debian GNU/Linux 12 (bookworm)
- Kernel: `6.1.0-31-amd64`
- Architecture: `x86_64`
- CPU: Intel(R) Xeon(R) CPU E5-2690 v4 @ 2.60GHz
- CPU cores: 4
- Memory: 3.8 GiB

Command:

```bash
KTP_BATCH_MATRIX_CLIENTS="1 2 4 8" \
KTP_BATCH_MATRIX_BATCHES="16 32 64" \
KTP_BATCH_MATRIX_RUNS=5 \
KTP_BATCH_MATRIX_FRAMES=64 \
KTP_BATCH_MATRIX_PAYLOAD_BYTES=8192 \
KTP_BATCH_MATRIX_CSV=/tmp/ktp-batch-matrix-clients-f960a42.csv \
  bash scripts/ktp-relay-batch-matrix.sh
```

Workload:

- Profile: `rdp-like`
- Runs: 5
- Clients: 1, 2, 4, 8
- Frames per client: 64
- Max payload: 8192 B
- Relay wait timeout: 100 us
- Relay batch sweep: 16, 32, 64

Relay batch sweep:

| Clients | Relay Batch Frames | Elapsed Median | Throughput Median | RTT p95 | RTT p99 | RTT Max | Relay Turns | Wait Turns | Max Batch In/Eg |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 16 | 15.612 ms | 3.777 MiB/s | 637 us | 1503 us | 2242 us | 760 | 750 | 1 / 2 |
| 1 | 32 | 16.951 ms | 3.478 MiB/s | 578 us | 1748 us | 2065 us | 786 | 779 | 1 / 2 |
| 1 | 64 | 16.392 ms | 3.597 MiB/s | 597 us | 2607 us | 5056 us | 774 | 766 | 1 / 2 |
| 2 | 16 | 27.916 ms | 4.224 MiB/s | 1481 us | 4879 us | 9040 us | 1411 | 1259 | 3 / 3 |
| 2 | 32 | 23.811 ms | 4.952 MiB/s | 747 us | 2353 us | 5066 us | 1219 | 1075 | 3 / 3 |
| 2 | 64 | 19.159 ms | 6.155 MiB/s | 670 us | 1684 us | 3870 us | 1205 | 1082 | 3 / 2 |
| 4 | 16 | 26.568 ms | 8.877 MiB/s | 1027 us | 2477 us | 5425 us | 1889 | 1453 | 6 / 4 |
| 4 | 32 | 29.290 ms | 8.052 MiB/s | 1191 us | 2705 us | 3574 us | 2011 | 1630 | 6 / 4 |
| 4 | 64 | 24.651 ms | 9.567 MiB/s | 1212 us | 2715 us | 10334 us | 1841 | 1456 | 7 / 4 |
| 8 | 16 | 63.979 ms | 7.372 MiB/s | 2677 us | 6148 us | 19081 us | 3080 | 2043 | 11 / 7 |
| 8 | 32 | 41.240 ms | 11.438 MiB/s | 1377 us | 2896 us | 5449 us | 3058 | 2099 | 9 / 8 |
| 8 | 64 | 31.126 ms | 15.154 MiB/s | 1184 us | 2583 us | 5242 us | 2965 | 2076 | 10 / 6 |

Notes:

- Single-client behavior is still noisy and does not justify tuning the global
  default downward. Batch 16 had the best median throughput, while batch 32 had
  the best p95 and max RTT in this sample.
- At 2 clients, batch 64 had the best median throughput and the best p95, p99,
  and max RTT.
- At 4 clients, batch 64 had the best median throughput and elapsed time, but
  the worst max RTT. Batch 16 had the better p95/p99 latency, while batch 32 had
  the best max RTT.
- At 8 clients, batch 64 was best across median elapsed time, median
  throughput, p95, p99, and max RTT. This is the strongest current evidence for
  keeping the relay batch cap at 64 for grouped or multi-session forwarding.
- Larger caps become meaningful as concurrency rises: observed max ingress
  batches grew from 1 frame at one client to 10 to 11 frames at eight clients.
  The next runtime optimization should focus on relay scheduling and fairness
  under bursty multi-session load, not on lowering the default batch cap.

## 2026-06-18 Fair Drain Scheduling Experiment

Code:

- Repository: `kelicloud-agent-rs`
- Base runtime commit: `f960a42` (`9dde393` only added benchmark notes and did
  not change runtime code)
- Experiment: change `AsyncTunnelCore::next_frames` from FIFO batch drain to a
  session-rotating fair drain that preserves per-session frame order.
- Result: not adopted. The experiment was removed before commit because it
  regressed the current batch 64 default under multi-client load.

Command:

```bash
KTP_BATCH_MATRIX_CLIENTS="1 2 4 8" \
KTP_BATCH_MATRIX_BATCHES="16 32 64" \
KTP_BATCH_MATRIX_RUNS=5 \
KTP_BATCH_MATRIX_FRAMES=64 \
KTP_BATCH_MATRIX_PAYLOAD_BYTES=8192 \
KTP_BATCH_MATRIX_CSV=/tmp/ktp-batch-matrix-fair-drain.csv \
  bash scripts/ktp-relay-batch-matrix.sh
```

Key comparison against the FIFO matrix above:

| Clients | Batch | FIFO Throughput Median | Fair Throughput Median | FIFO RTT p95 | Fair RTT p95 | FIFO RTT Max | Fair RTT Max |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 2 | 64 | 6.155 MiB/s | 6.217 MiB/s | 670 us | 766 us | 3870 us | 3648 us |
| 4 | 64 | 9.567 MiB/s | 4.957 MiB/s | 1212 us | 2141 us | 10334 us | 12366 us |
| 8 | 64 | 15.154 MiB/s | 13.561 MiB/s | 1184 us | 1416 us | 5242 us | 4949 us |

Notes:

- Session-rotating fair drain improved some smaller-batch samples, but it hurt
  the production candidate path: batch 64 with four and eight clients.
- The four-client batch 64 sample is the clearest rejection signal: median
  throughput dropped from 9.567 MiB/s to 4.957 MiB/s and p95 latency rose from
  1212 us to 2141 us.
- Do not replace FIFO batch drain with naive per-session round-robin drain.
  The next scheduling attempt should first add per-session latency or queue
  dwell diagnostics, then test an adaptive policy that avoids reordering the
  entire outbound queue under high batch caps.

## 2026-06-18 Per-Client RTT Fairness Diagnostics

Code:

- Repository: `kelicloud-agent-rs`
- Base runtime commit: `d865b66`
- Change: `ktp-e2e-bench --latency` now reports per-client RTT fairness fields,
  and `scripts/ktp-relay-batch-matrix.sh` writes them into CSV output.

New fields:

- `rtt_client_p95_micros_min`
- `rtt_client_p95_micros_max`
- `rtt_client_p95_spread_micros`
- `rtt_client_max_micros_max`

Smoke command:

```bash
KTP_BATCH_MATRIX_CLIENTS="2" \
KTP_BATCH_MATRIX_BATCHES="64" \
KTP_BATCH_MATRIX_RUNS=2 \
KTP_BATCH_MATRIX_FRAMES=16 \
KTP_BATCH_MATRIX_PAYLOAD_BYTES=1024 \
KTP_BATCH_MATRIX_CSV=/tmp/ktp-client-fairness-smoke.csv \
  bash scripts/ktp-relay-batch-matrix.sh
```

Smoke result:

| Clients | Batch | RTT p95 | Client p95 Min | Client p95 Max | Client p95 Spread | Client Max |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 2 | 64 | 966 us | 966 us | 2944 us | 1978 us | 6952 us |

Notes:

- Global p95 alone can hide a slow client. In the smoke sample, global p95 was
  966 us while the slowest client's p95 reached 2944 us.
- Future relay scheduling experiments should compare both aggregate throughput
  and `rtt_client_p95_spread_micros`. A candidate that raises client spread
  materially is not a clear win even if global p95 looks acceptable.

## 2026-06-18 Client Fairness Matrix

Code:

- Repository: `kelicloud-agent-rs`
- Commit: `71f379f`
- End-to-end binary: `ktp-e2e-bench`
- Build mode: release builds created by `scripts/ktp-relay-batch-matrix.sh`
- Run directory: `/root/kelicloud-agent-rs-fairness-71f379f-a`

Command:

```bash
KTP_BATCH_MATRIX_CLIENTS="1 2 4 8" \
KTP_BATCH_MATRIX_BATCHES="16 32 64" \
KTP_BATCH_MATRIX_RUNS=5 \
KTP_BATCH_MATRIX_FRAMES=64 \
KTP_BATCH_MATRIX_PAYLOAD_BYTES=8192 \
KTP_BATCH_MATRIX_CSV=/tmp/ktp-client-fairness-matrix-71f379f.csv \
  bash scripts/ktp-relay-batch-matrix.sh
```

Fairness matrix:

| Clients | Batch | Throughput Median | RTT p95 | RTT p99 | RTT Max | Client p95 Spread |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 16 | 3.662 MiB/s | 478 us | 1769 us | 2112 us | 0 us |
| 1 | 32 | 4.122 MiB/s | 501 us | 1266 us | 3053 us | 0 us |
| 1 | 64 | 4.250 MiB/s | 407 us | 1115 us | 1396 us | 0 us |
| 2 | 16 | 6.236 MiB/s | 757 us | 2430 us | 3038 us | 28 us |
| 2 | 32 | 6.353 MiB/s | 681 us | 1588 us | 2994 us | 98 us |
| 2 | 64 | 7.032 MiB/s | 905 us | 1466 us | 5037 us | 48 us |
| 4 | 16 | 5.805 MiB/s | 983 us | 3711 us | 7943 us | 634 us |
| 4 | 32 | 12.046 MiB/s | 606 us | 1253 us | 3323 us | 70 us |
| 4 | 64 | 9.661 MiB/s | 828 us | 1927 us | 3565 us | 270 us |
| 8 | 16 | 11.638 MiB/s | 1538 us | 3144 us | 5557 us | 400 us |
| 8 | 32 | 11.744 MiB/s | 1713 us | 4333 us | 23893 us | 734 us |
| 8 | 64 | 11.899 MiB/s | 1519 us | 4738 us | 26134 us | 884 us |

Notes:

- Batch 64 remains the best single-client sample and has the highest median
  throughput at two and eight clients, but fairness changes the tuning picture.
- At four clients, batch 32 is the best balanced point: highest median
  throughput, lowest global p95/p99, and the lowest client p95 spread.
- At eight clients, batch 64 only improves median throughput by about 2.2% over
  batch 16, while client p95 spread more than doubles and max RTT rises from
  5557 us to 26134 us.
- The next adaptive scheduling experiment should not be a global FIFO
  replacement. A safer shape is a concurrency-aware batch cap: keep larger
  batches for low concurrency, then reduce the effective drain cap when active
  sessions are high or client p95 spread grows.

## 2026-06-18 KTP Adaptive Batch Policy Smoke

Code:

- Repository: `kelicloud-agent-rs`
- Base commit: `f39a882`
- Patch: benchmark-only `--relay-batch-policy fixed|adaptive`
- End-to-end binary: `ktp-e2e-bench`
- Run directory: `/root/kelicloud-agent-rs-adaptive-batch-20260618a`

Command shape:

```bash
KTP_BATCH_MATRIX_CLIENTS="4 8" \
KTP_BATCH_MATRIX_BATCHES="64" \
KTP_BATCH_MATRIX_BATCH_POLICY=<fixed|adaptive> \
KTP_BATCH_MATRIX_RUNS=3 \
KTP_BATCH_MATRIX_FRAMES=64 \
KTP_BATCH_MATRIX_PAYLOAD_BYTES=8192 \
KTP_BATCH_MATRIX_CSV=/tmp/ktp-adaptive-<policy>-smoke.csv \
  bash scripts/ktp-relay-batch-matrix.sh
```

Smoke comparison:

| Policy | Clients | Configured Batch | Effective Batch | Throughput Median | RTT p95 | RTT p99 | RTT Max | Client p95 Spread |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| fixed | 4 | 64 | 64 | 9.373 MiB/s | 1127 us | 1892 us | 2641 us | 181 us |
| adaptive | 4 | 64 | 32 | 12.219 MiB/s | 499 us | 1150 us | 2898 us | 221 us |
| fixed | 8 | 64 | 64 | 16.648 MiB/s | 1015 us | 1898 us | 3455 us | 389 us |
| adaptive | 8 | 64 | 16 | 15.988 MiB/s | 959 us | 1670 us | 2963 us | 532 us |

Notes:

- The adaptive policy is intentionally benchmark-only in this patch. Production
  runtime behavior remains fixed unless a future runtime change opts in.
- At four clients, the adaptive cap improved median throughput and global RTT
  p95/p99 on this smoke run.
- At eight clients, adaptive reduced global p95/p99/max but slightly reduced
  median throughput and widened client p95 spread. This needs a larger matrix
  before becoming the default scheduling policy.

Follow-up tool change:

- `scripts/ktp-relay-batch-matrix.sh` now accepts
  `KTP_BATCH_MATRIX_BATCH_POLICIES="fixed adaptive"` so the same command can
  produce a single CSV containing both policy rows. The previous
  `KTP_BATCH_MATRIX_BATCH_POLICY=<policy>` path remains supported for single
  policy runs.

Validation command:

```bash
KTP_BATCH_MATRIX_CLIENTS="4" \
KTP_BATCH_MATRIX_BATCHES="64" \
KTP_BATCH_MATRIX_BATCH_POLICIES="fixed adaptive" \
KTP_BATCH_MATRIX_RUNS=2 \
KTP_BATCH_MATRIX_FRAMES=64 \
KTP_BATCH_MATRIX_PAYLOAD_BYTES=8192 \
KTP_BATCH_MATRIX_CSV=/tmp/ktp-policy-compare-smoke.csv \
  bash scripts/ktp-relay-batch-matrix.sh
```

Validation output:

| Policy | Clients | Configured Batch | Effective Batch | Throughput Median | RTT p95 | RTT p99 | RTT Max | Client p95 Spread |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| fixed | 4 | 64 | 64 | 9.331 MiB/s | 949 us | 2940 us | 5768 us | 248 us |
| adaptive | 4 | 64 | 32 | 5.604 MiB/s | 1456 us | 3654 us | 8002 us | 2058 us |

This two-run smoke confirms the compare path and CSV shape. It also shows why
larger same-host matrices are required before promoting adaptive scheduling:
the short sample contradicted the earlier three-run smoke at four clients.

Policy summary gate:

- `ktp-policy-summary <csv>` reads a relay batch matrix CSV and pairs fixed and
  adaptive rows by `clients` and `relay_batch_frames`.
- Verdicts are intentionally conservative:
  - `same_effective` when fixed and adaptive produce the same effective batch
    cap, because any measured difference is not caused by a scheduling change.
  - `adaptive_better` only when median throughput is no worse, RTT p95 is no
    worse, and client p95 spread is no worse.
  - `fixed_better` when adaptive regresses all three primary metrics.
  - `mixed` for trade-offs that require human review before runtime changes.

Validation command:

```bash
cargo run --release --bin ktp-policy-summary -- /tmp/ktp-policy-compare-smoke.csv
```

Validation output:

```text
ktp_policy_summary rows=2 pairs=1
clients=4 relay_batch_frames=64 fixed_effective=64 adaptive_effective=32 throughput_delta_pct=-39.94 rtt_p95_delta_pct=53.42 client_p95_spread_delta_pct=729.84 verdict=fixed_better
```

Gate command:

```bash
cargo run --release --bin ktp-policy-summary -- \
  --fail-on-fixed-better /tmp/ktp-policy-compare-smoke.csv
```

The gate still prints the summary, then exits non-zero if any pair is
`verdict=fixed_better`. The validation sample above produced
`gate_exit_code=3`, so this gate can be used to prevent a candidate scheduling
policy from being promoted when it loses throughput, RTT p95, and client p95
fairness at the same time.

The matrix script can run the same gate automatically after all CSV rows are
written:

```bash
KTP_BATCH_MATRIX_BATCH_POLICIES="fixed adaptive" \
KTP_BATCH_MATRIX_CSV=/tmp/ktp-policy-compare.csv \
KTP_BATCH_MATRIX_FAIL_ON_FIXED_BETTER=1 \
bash scripts/ktp-relay-batch-matrix.sh
```

`KTP_BATCH_MATRIX_FAIL_ON_FIXED_BETTER=1` requires `KTP_BATCH_MATRIX_CSV` in
non-dry-run mode. The script runs `ktp-policy-summary --fail-on-fixed-better`
against that CSV and returns the summary exit code.

The relay batch matrix CSV includes `client_payload_reused` from
`ktp-e2e-bench`; current runtime e2e samples should report `1` because the
benchmark client reuses a preallocated payload buffer for each frame.

Linux runtime payload-reuse smoke sample:

```text
profile,runs,clients,frames,payload_bytes,client_payload_reused,relay_batch_frames,relay_batch_policy,relay_batch_frames_effective,elapsed_ms_min,elapsed_ms_median,elapsed_ms_max,throughput_mib_s_min,throughput_mib_s_median,throughput_mib_s_max,rtt_micros_p50,rtt_micros_p95,rtt_micros_p99,rtt_micros_max,rtt_client_p95_micros_min,rtt_client_p95_micros_max,rtt_client_p95_spread_micros,rtt_client_max_micros_max,relay_turns,relay_wait_turns,ingress_batches,egress_batches,ingress_max_batch_frames,egress_max_batch_frames
rdp-like,1,1,8,1024,1,4,fixed,4,4.162,4.162,4.162,0.337,0.337,0.337,206,1042,1042,1042,1042,1042,0,1042,28,26,9,8,1,2
```

The local tunnel smoke keeps this policy gate optional so ordinary smoke runs
do not fail while a candidate adaptive policy is still being tuned:

```bash
KTP_SMOKE_POLICY_GATE=1 bash scripts/tunnel-relay-local-smoke.sh
```

When enabled, the smoke script writes the matrix CSV to
`${TMPDIR:-/tmp}/ktp-relay-policy-smoke.csv` unless `KTP_SMOKE_POLICY_CSV`
overrides it.

Conservative adaptive cap update:

- The initial adaptive smoke used `clients >= 4 => cap 32` and
  `clients >= 8 => cap 16`. Later repeated two-run samples showed the
  four-client cap could lose throughput, RTT p95, and client p95 spread at the
  same time.
- The candidate policy is now deliberately less aggressive:
  - fewer than 8 clients: keep the configured batch cap;
  - 8 to 15 clients: cap at 32 frames;
  - 16 or more clients: cap at 16 frames.
- This keeps the low-concurrency RDP-like case on the known-good fixed batch
  path while still leaving a measurable adaptive branch for higher fan-out
  experiments.

Validation after the conservative cap:

```bash
KTP_BATCH_MATRIX_BATCH_POLICIES="fixed adaptive" \
KTP_BATCH_MATRIX_CLIENTS=4 \
KTP_BATCH_MATRIX_BATCHES=64 \
KTP_BATCH_MATRIX_RUNS=5 \
KTP_BATCH_MATRIX_CSV=/tmp/ktp-policy-conservative-smoke.csv \
KTP_BATCH_MATRIX_FAIL_ON_FIXED_BETTER=1 \
bash scripts/ktp-relay-batch-matrix.sh
```

Summary:

```text
clients=4 relay_batch_frames=64 fixed_effective=64 adaptive_effective=64 throughput_delta_pct=17.48 rtt_p95_delta_pct=-28.70 client_p95_spread_delta_pct=-48.73 verdict=same_effective
KTP_SMOKE_POLICY_GATE=1 bash scripts/tunnel-relay-local-smoke.sh completed successfully.
```

Encrypted carrier repeated-run statistics:

- `ktp-tunnel-bench --runs N` now reports per-run min/median/max elapsed time
  and throughput when `N > 1`, matching the evidence shape used by
  `ktp-e2e-bench`.
- `scripts/tunnel-relay-local-smoke.sh` runs the encrypted TCP carrier bench
  with `KTP_SMOKE_CARRIER_RUNS=3` by default. The value can be overridden for a
  longer local soak without changing the script.
- Single-run output still keeps the compact `elapsed_ms` and
  `throughput_mib_s` fields for quick developer checks.

Tunnel-data receive batch foundation:

- The tunnel-data socket trait now has an optional KTP-frame batch read path.
  Default transports decode one frame at a time, preserving WebSocket
  compatibility.
- The encrypted TCP tunnel-data socket overrides that path and can deliver
  multiple already-buffered KTP frames to the session loop in one read step.
- This does not change the KTP frame format or backend schema. It reduces
  per-frame loop overhead on the dedicated KTP TCP carrier and should be paired
  with future before/after benchmark evidence before claiming throughput gains.
- `ktp-tunnel-bench --direction relay-to-client-batch-read` exercises the
  dedicated KTP TCP relay-to-agent path through the tunnel-data socket's batch
  read interface and reports `read_batch_frames=64`.
- `scripts/tunnel-relay-local-smoke.sh` runs a small batch-read carrier sample
  by default. Use `KTP_SMOKE_BATCH_READ_FRAMES` and
  `KTP_SMOKE_BATCH_READ_PAYLOAD_BYTES` to adjust that smoke workload.
- KTP encrypted TCP streams enable `TCP_NODELAY` by default. That keeps
  interactive RDP-like payloads from waiting behind Nagle coalescing while the
  existing batched write path still coalesces intentional KTP frame batches.
- The conservative `fixed|adaptive` relay batch policy is now shared by
  benchmark tooling and the production tunnel runtime. Production defaults stay
  `fixed`; `adaptive` is an explicit runtime-limit choice so it can be tested or
  rolled back without changing the KTP frame format, backend schema, or default
  WebSocket behavior. Operators can opt into it with
  `--tunnel-ktp-relay-batch-policy adaptive` or
  `AGENT_TUNNEL_KTP_RELAY_BATCH_POLICY=adaptive`.

Linux debug smoke sample:

```text
ktp_tunnel_bench carrier=encrypted_tcp direction=relay_to_client_batch_read runs=1 frames=512 payload_bytes=4096 bytes=2097152 bytes_per_run=2097152 total_bytes=2097152 read_batch_frames=64 elapsed_ms=976.214 throughput_mib_s=2.049
```

This sample proves the smoke path and metric shape on the release host. It is
not a release throughput baseline because the smoke uses a debug build and a
small workload.

Carrier direction matrix:

```bash
KTP_CARRIER_MATRIX_CSV=/tmp/ktp-carrier-matrix.csv \
  bash scripts/ktp-carrier-matrix.sh
```

The matrix runs `ktp-tunnel-bench` in release mode across carrier directions,
frame counts, and payload sizes. It writes a CSV with direction, run count,
payload shape, optional `write_batch_frames`, optional `read_batch_frames`, and
min/median/max elapsed and throughput fields. The default direction sweep
compares one-frame client writes, client-to-relay batch writes, and
relay-to-client batch reads. Use this for carrier-layer before/after
comparisons; keep runtime/RDP fairness comparisons in
`scripts/ktp-relay-batch-matrix.sh`. The client-to-relay batch-write and
relay-to-client batch-read benchmarks prebuild reusable frame batches before
the timed section; the direct `ktp-tunnel-bench` output includes
`write_batch_reused=1` or `read_batch_reused=1` for those directions so carrier
samples are not dominated by per-batch payload cloning.

Linux release smoke sample:

```text
direction,runs,frames,payload_bytes,write_batch_frames,write_batch_reused,read_batch_frames,read_batch_reused,elapsed_ms_min,elapsed_ms_median,elapsed_ms_max,throughput_mib_s_min,throughput_mib_s_median,throughput_mib_s_max
client_to_relay,2,64,1024,0,0,0,0,2.137,2.298,2.458,25.428,27.337,29.245
client_to_relay_batch_write,2,64,1024,64,1,0,0,1.177,1.283,1.390,44.970,49.034,53.099
relay_to_client_batch_read,2,64,1024,0,0,64,1,0.437,0.484,0.532,117.533,130.262,142.990
```

This sample was intentionally small so it can run as a quick release-mode
verification. Larger matrices should raise frame counts, payload sizes, and run
count before being used as tuning evidence. In this short run, reusable
client-to-relay batch writes improved median throughput over one-frame writes,
but the sample is still too small to promote a production tuning claim by
itself.

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

Carrier matrix smoke:

```bash
KELICLOUD_LOCAL_BACKEND_MATRIX_LOG_DIR=/tmp/kelicloud-local-backend-matrix/logs \
KELICLOUD_LOCAL_BACKEND_MATRIX_WORK_DIR=/tmp/kelicloud-local-backend-matrix/work \
  bash scripts/ktp-local-backend-matrix.sh
```

The matrix wrapper runs the same local backend smoke once through the default
WebSocket tunnel-data carrier and once through the KTP TCP carrier. Each run
gets an isolated log directory, so the KTP evidence file and `agent.summary.md`
can be compared without mixing artifacts.

The wrapper also writes `matrix-summary.tsv` under the matrix log directory by
default. The TSV records `carrier`, whether KTP TCP was enabled, pass/fail
status, the run log directory, `agent.summary.md`, and the KTP live-canary
evidence path when that carrier produces one.

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

- Higher sample-count multi-client runs, including at least one 8+ client
  release-host matrix after the next relay scheduling change, with
  `rtt_client_p95_spread_micros` included in the comparison.
- Longer release-host and live-canary samples for
  `ktp-e2e-bench --profile rdp-like` so tunnel tuning compares interactive
  mixed-payload traffic instead of fixed-size frames only.
- Before/after diagnostics for production data carrier scheduling changes.
- Inspect the next GitHub Actions KTP local-backend artifact and keep it as the
  release evidence source instead of relying on one-off remote host paths.
- Live KTP canary traffic with real RDP-like forwarding and paired RTT or
  throughput evidence from the same observation window.
