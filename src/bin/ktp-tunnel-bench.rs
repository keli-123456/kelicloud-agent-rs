use kelicloud_agent_rs::ktp::{FrameLeg, FrameType, KtpFrame, KTP_MAX_PAYLOAD_LEN};
use kelicloud_agent_rs::ktp_transport::{KtpCryptoDirection, KtpCryptoKey, KtpEncryptedTcpStream};
use std::error::Error;
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, TcpStream};

type BenchResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Copy, Debug)]
struct BenchConfig {
    frames: usize,
    payload_bytes: usize,
    runs: usize,
}

#[derive(Clone, Copy, Debug)]
struct BenchSample {
    bytes: usize,
    elapsed: Duration,
}

fn main() {
    let config = match parse_args(std::env::args().skip(1)) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            print_usage();
            std::process::exit(2);
        }
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_io()
        .enable_time()
        .build()
        .expect("build ktp tunnel bench runtime");

    match runtime.block_on(run_benchmark(config)) {
        Ok(report) => println!("{report}"),
        Err(error) => {
            eprintln!("ktp-tunnel-bench failed: {error}");
            std::process::exit(1);
        }
    }
}

fn parse_args(args: impl Iterator<Item = String>) -> BenchResult<BenchConfig> {
    let mut frames = 4096usize;
    let mut payload_bytes = 16 * 1024usize;
    let mut runs = 1usize;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--frames" => {
                frames = parse_positive_usize(next_value(&mut args, "--frames")?, "--frames")?
            }
            "--payload-bytes" => {
                payload_bytes = parse_positive_usize(
                    next_value(&mut args, "--payload-bytes")?,
                    "--payload-bytes",
                )?
            }
            "--runs" => runs = parse_positive_usize(next_value(&mut args, "--runs")?, "--runs")?,
            "--help" | "-h" => return Err("help requested".into()),
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    if payload_bytes > KTP_MAX_PAYLOAD_LEN {
        return Err(format!(
            "--payload-bytes must be <= {KTP_MAX_PAYLOAD_LEN}, got {payload_bytes}"
        )
        .into());
    }
    Ok(BenchConfig {
        frames,
        payload_bytes,
        runs,
    })
}

fn next_value(
    args: &mut std::iter::Peekable<impl Iterator<Item = String>>,
    flag: &str,
) -> BenchResult<String> {
    args.next()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{flag} requires a value").into())
}

fn parse_positive_usize(raw: String, flag: &str) -> BenchResult<usize> {
    let value = raw
        .parse::<usize>()
        .map_err(|_| format!("{flag} must be a positive integer"))?;
    if value == 0 {
        return Err(format!("{flag} must be greater than zero").into());
    }
    Ok(value)
}

async fn run_benchmark(config: BenchConfig) -> BenchResult<String> {
    let mut samples = Vec::with_capacity(config.runs);
    for _ in 0..config.runs {
        samples.push(run_benchmark_once(config).await?);
    }
    let bytes_per_run = config.frames * config.payload_bytes;
    let total_bytes = samples.iter().map(|sample| sample.bytes).sum::<usize>();
    if samples.len() == 1 {
        let sample = samples
            .first()
            .expect("single-run benchmark should have one sample");
        let elapsed_secs = sample.elapsed.as_secs_f64().max(0.000_001);
        let throughput_mib_s = (sample.bytes as f64 / (1024.0 * 1024.0)) / elapsed_secs;
        return Ok(format!(
            "ktp_tunnel_bench carrier=encrypted_tcp direction=client_to_relay runs={} frames={} payload_bytes={} bytes={} bytes_per_run={} total_bytes={} elapsed_ms={:.3} throughput_mib_s={:.3}",
            config.runs,
            config.frames,
            config.payload_bytes,
            bytes_per_run,
            bytes_per_run,
            total_bytes,
            sample.elapsed.as_secs_f64() * 1000.0,
            throughput_mib_s
        ));
    }

    let mut elapsed_values = samples
        .iter()
        .map(|sample| sample.elapsed.as_secs_f64() * 1000.0)
        .collect::<Vec<_>>();
    elapsed_values.sort_by(f64::total_cmp);
    let mut throughput_values = samples
        .iter()
        .map(|sample| {
            let elapsed_secs = sample.elapsed.as_secs_f64().max(0.000_001);
            (sample.bytes as f64 / (1024.0 * 1024.0)) / elapsed_secs
        })
        .collect::<Vec<_>>();
    throughput_values.sort_by(f64::total_cmp);

    Ok(format!(
        "ktp_tunnel_bench carrier=encrypted_tcp direction=client_to_relay runs={} frames={} payload_bytes={} bytes={} bytes_per_run={} total_bytes={} elapsed_ms_min={:.3} elapsed_ms_median={:.3} elapsed_ms_max={:.3} throughput_mib_s_min={:.3} throughput_mib_s_median={:.3} throughput_mib_s_max={:.3}",
        config.runs,
        config.frames,
        config.payload_bytes,
        bytes_per_run,
        bytes_per_run,
        total_bytes,
        elapsed_values[0],
        median(&elapsed_values),
        elapsed_values[elapsed_values.len() - 1],
        throughput_values[0],
        median(&throughput_values),
        throughput_values[throughput_values.len() - 1],
    ))
}

async fn run_benchmark_once(config: BenchConfig) -> BenchResult<BenchSample> {
    let key = KtpCryptoKey::from_bytes([0x42; 32]);
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server_key = key.clone();
    let frames = config.frames;
    let payload_bytes = config.payload_bytes;

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let mut stream = KtpEncryptedTcpStream::from_stream(
            stream,
            server_key,
            KtpCryptoDirection::RelayToClient,
            KtpCryptoDirection::ClientToRelay,
            KTP_MAX_PAYLOAD_LEN,
            4 * 1024 * 1024,
        );
        let mut bytes = 0usize;
        for _ in 0..frames {
            let frame = stream.next_frame().await?;
            bytes += frame.payload.len();
        }
        BenchResult::Ok(bytes)
    });

    let stream = TcpStream::connect(address).await?;
    let mut client = KtpEncryptedTcpStream::from_stream(
        stream,
        key,
        KtpCryptoDirection::ClientToRelay,
        KtpCryptoDirection::RelayToClient,
        KTP_MAX_PAYLOAD_LEN,
        4 * 1024 * 1024,
    );
    let frame = KtpFrame {
        frame_type: FrameType::SessionData,
        leg: FrameLeg::Ingress,
        flags: 0,
        session_id: 1,
        payload: vec![0x5a; payload_bytes],
    };
    let started = Instant::now();
    for _ in 0..config.frames {
        client.send_frame(&frame).await?;
    }
    drop(client);
    let bytes = server.await??;
    let elapsed = started.elapsed();
    Ok(BenchSample { bytes, elapsed })
}

fn print_usage() {
    eprintln!("usage: ktp-tunnel-bench [--frames N] [--payload-bytes BYTES] [--runs N]");
}

fn median(sorted_values: &[f64]) -> f64 {
    let middle = sorted_values.len() / 2;
    if sorted_values.len() % 2 == 0 {
        (sorted_values[middle - 1] + sorted_values[middle]) / 2.0
    } else {
        sorted_values[middle]
    }
}
