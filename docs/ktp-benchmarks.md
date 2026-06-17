# KTP Benchmark Notes

This file records repeatable KTP data-plane benchmark evidence. Treat these
numbers as engineering baselines, not production capacity promises.

## 2026-06-17 Linux Release Baseline

Code:

- Repository: `kelicloud-agent-rs`
- Commit: `9fe3b83`
- Binary: `ktp-tunnel-bench`
- Build mode: `cargo build --release --bin ktp-tunnel-bench`

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

| Frames | Payload | Total Bytes | Elapsed | Throughput |
| ---: | ---: | ---: | ---: | ---: |
| 65536 | 1024 B | 201326592 | 1258.015 ms | 152.621 MiB/s |
| 4096 | 16384 B | 201326592 | 647.507 ms | 296.522 MiB/s |
| 2048 | 65536 B | 402653184 | 1084.613 ms | 354.043 MiB/s |

Observations:

- Small 1 KiB frames are dominated by per-frame overhead.
- 16 KiB and 64 KiB payloads show much higher encrypted TCP carrier throughput.
- Future optimization should focus on frame batching, read/write scheduling,
  and end-to-end relay behavior before changing cryptography.

Next evidence to collect:

- Same benchmark on a release Linux host with CPU details captured.
- End-to-end ingress-to-egress tunnel throughput, not only encrypted carrier
  write/read throughput.
- Latency distribution for small frames.
