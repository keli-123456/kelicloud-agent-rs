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

Next evidence to collect:

- Same benchmark on a release Linux host with CPU details captured.
- Repeated multi-client runs with higher sample counts and percentile summaries.
- Latency distribution for small frames.
- Integrating shared readiness into the production data carrier scheduling path,
  not only the benchmark relay.
- Before/after diagnostics for production data carrier scheduling changes.
