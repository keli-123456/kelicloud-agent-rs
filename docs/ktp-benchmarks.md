# KTP Benchmark Notes

This file records repeatable KTP data-plane benchmark evidence. Treat these
numbers as engineering baselines, not production capacity promises.

## 2026-06-17 Linux Release Baseline

Code:

- Repository: `kelicloud-agent-rs`
- Commit: `9fe3b83` for carrier results
- Commit: `fcf21a8` for end-to-end runtime results with batched frame drain
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

Observations:

- Small 1 KiB frames are dominated by per-frame overhead.
- 16 KiB and 64 KiB payloads show much higher encrypted TCP carrier throughput.
- End-to-end runtime throughput is still far below carrier-only throughput, so
  the next bottleneck is runtime relay scheduling and per-frame session
  handling, not ChaCha20-Poly1305 encryption itself.
- Batched runtime frame drain keeps the large-payload path around the previous
  baseline and improves the documented 1 KiB baseline, but it is only the first
  step. Future optimization should focus on read/write scheduling, multi-session
  relay fairness, and reducing ingress/egress relay round trips before changing
  cryptography.

Next evidence to collect:

- Same benchmark on a release Linux host with CPU details captured.
- Multi-connection end-to-end ingress-to-egress throughput.
- Latency distribution for small frames.
