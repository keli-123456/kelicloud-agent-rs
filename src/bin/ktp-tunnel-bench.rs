use kelicloud_agent_rs::ktp::{FrameLeg, FrameType, KtpFrame, KTP_MAX_PAYLOAD_LEN};
use kelicloud_agent_rs::ktp_transport::{KtpCryptoDirection, KtpCryptoKey, KtpEncryptedTcpStream};
use kelicloud_agent_rs::tunnel_data::{
    KtpEncryptedTcpTunnelDataTransport, TunnelDataSocket, TunnelDataTransport,
};
use std::error::Error;
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, TcpStream};

type BenchResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Copy, Debug)]
struct BenchConfig {
    frames: usize,
    payload_bytes: usize,
    runs: usize,
    direction: BenchDirection,
}

#[derive(Clone, Copy, Debug)]
struct BenchSample {
    bytes: usize,
    elapsed: Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BenchDirection {
    ClientToRelay,
    ClientToRelayBatchWrite,
    RelayToClientBatchRead,
}

impl BenchDirection {
    fn parse(value: &str) -> BenchResult<Self> {
        match value {
            "client-to-relay" => Ok(Self::ClientToRelay),
            "client-to-relay-batch-write" => Ok(Self::ClientToRelayBatchWrite),
            "relay-to-client-batch-read" => Ok(Self::RelayToClientBatchRead),
            _ => Err("--direction must be client-to-relay, client-to-relay-batch-write, or relay-to-client-batch-read".into()),
        }
    }

    fn report_value(self) -> &'static str {
        match self {
            Self::ClientToRelay => "client_to_relay",
            Self::ClientToRelayBatchWrite => "client_to_relay_batch_write",
            Self::RelayToClientBatchRead => "relay_to_client_batch_read",
        }
    }
}

const WRITE_BATCH_FRAMES: usize = 64;
const READ_BATCH_FRAMES: usize = 64;
const BENCH_CARRIER: &str = "ktp_tcp";
const BENCH_CRYPTO: &str = "ktp_aead";

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
    let mut direction = BenchDirection::ClientToRelay;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--direction" => {
                direction = BenchDirection::parse(&next_value(&mut args, "--direction")?)?
            }
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
        direction,
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
            "ktp_tunnel_bench carrier={} crypto={} direction={} runs={} frames={} payload_bytes={} bytes={} bytes_per_run={} total_bytes={}{} elapsed_ms={:.3} throughput_mib_s={:.3}",
            BENCH_CARRIER,
            BENCH_CRYPTO,
            config.direction.report_value(),
            config.runs,
            config.frames,
            config.payload_bytes,
            bytes_per_run,
            bytes_per_run,
            total_bytes,
            batch_direction_suffix(config.direction),
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
        "ktp_tunnel_bench carrier={} crypto={} direction={} runs={} frames={} payload_bytes={} bytes={} bytes_per_run={} total_bytes={}{} elapsed_ms_min={:.3} elapsed_ms_median={:.3} elapsed_ms_max={:.3} throughput_mib_s_min={:.3} throughput_mib_s_median={:.3} throughput_mib_s_max={:.3}",
        BENCH_CARRIER,
        BENCH_CRYPTO,
        config.direction.report_value(),
        config.runs,
        config.frames,
        config.payload_bytes,
        bytes_per_run,
        bytes_per_run,
        total_bytes,
        batch_direction_suffix(config.direction),
        elapsed_values[0],
        median(&elapsed_values),
        elapsed_values[elapsed_values.len() - 1],
        throughput_values[0],
        median(&throughput_values),
        throughput_values[throughput_values.len() - 1],
    ))
}

async fn run_benchmark_once(config: BenchConfig) -> BenchResult<BenchSample> {
    match config.direction {
        BenchDirection::ClientToRelay => run_client_to_relay_benchmark_once(config).await,
        BenchDirection::ClientToRelayBatchWrite => {
            run_client_to_relay_batch_write_benchmark_once(config).await
        }
        BenchDirection::RelayToClientBatchRead => {
            run_relay_to_client_batch_read_benchmark_once(config).await
        }
    }
}

async fn run_client_to_relay_benchmark_once(config: BenchConfig) -> BenchResult<BenchSample> {
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

async fn run_client_to_relay_batch_write_benchmark_once(
    config: BenchConfig,
) -> BenchResult<BenchSample> {
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
    let batches = reusable_frame_batches(&frame, config.frames, WRITE_BATCH_FRAMES);
    let started = Instant::now();
    for batch in &batches {
        client.send_frames(batch).await?;
    }
    drop(client);
    let bytes = server.await??;
    let elapsed = started.elapsed();
    Ok(BenchSample { bytes, elapsed })
}

async fn run_relay_to_client_batch_read_benchmark_once(
    config: BenchConfig,
) -> BenchResult<BenchSample> {
    let key = KtpCryptoKey::from_bytes([0x42; 32]);
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server_key = key.clone();
    let payload_bytes = config.payload_bytes;
    let frame = KtpFrame {
        frame_type: FrameType::SessionData,
        leg: FrameLeg::Egress,
        flags: 0,
        session_id: 1,
        payload: vec![0x5a; payload_bytes],
    };
    let batches = reusable_frame_batches(&frame, config.frames, READ_BATCH_FRAMES);

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
        for batch in &batches {
            stream.send_frames(batch).await?;
        }
        BenchResult::Ok(())
    });

    let client = tokio::task::spawn_blocking(move || {
        let mut transport = KtpEncryptedTcpTunnelDataTransport::new(key);
        let mut socket = transport.connect_tunnel_data(&format!("ktp+tcp://{address}"), &[])?;
        let started = Instant::now();
        let mut bytes = 0usize;
        let mut received_frames = 0usize;
        while received_frames < config.frames {
            let Some(frames) = socket.read_optional_ktp_frame_batch(READ_BATCH_FRAMES)? else {
                return Err("timed out waiting for relay-to-client batch frames".into());
            };
            if frames.is_empty() {
                return Err("empty relay-to-client batch frame read".into());
            }
            for frame in frames {
                bytes += frame.payload.len();
                received_frames += 1;
            }
        }
        let elapsed = started.elapsed();
        drop(socket);
        BenchResult::Ok(BenchSample { bytes, elapsed })
    });

    let sample = client.await??;
    server.await??;
    Ok(sample)
}

fn print_usage() {
    eprintln!("usage: ktp-tunnel-bench [--direction client-to-relay|client-to-relay-batch-write|relay-to-client-batch-read] [--frames N] [--payload-bytes BYTES] [--runs N]");
}

fn median(sorted_values: &[f64]) -> f64 {
    let middle = sorted_values.len() / 2;
    if sorted_values.len() % 2 == 0 {
        (sorted_values[middle - 1] + sorted_values[middle]) / 2.0
    } else {
        sorted_values[middle]
    }
}

fn batch_direction_suffix(direction: BenchDirection) -> String {
    match direction {
        BenchDirection::ClientToRelay => String::new(),
        BenchDirection::ClientToRelayBatchWrite => {
            format!(" write_batch_frames={WRITE_BATCH_FRAMES} write_batch_reused=1")
        }
        BenchDirection::RelayToClientBatchRead => {
            format!(" read_batch_frames={READ_BATCH_FRAMES} read_batch_reused=1")
        }
    }
}

fn reusable_frame_batches(
    frame: &KtpFrame,
    frame_count: usize,
    batch_size: usize,
) -> Vec<Vec<KtpFrame>> {
    let batch_size = batch_size.max(1);
    let mut remaining = frame_count;
    let mut batches = Vec::with_capacity(frame_count.div_ceil(batch_size));
    while remaining > 0 {
        let chunk = remaining.min(batch_size);
        batches.push((0..chunk).map(|_| frame.clone()).collect());
        remaining -= chunk;
    }
    batches
}
