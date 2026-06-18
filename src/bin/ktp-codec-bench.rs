use kelicloud_agent_rs::ktp::{
    encode_frame, FrameLeg, FrameType, KtpFrame, KTP_HEADER_LEN, KTP_MAX_PAYLOAD_LEN,
};
use kelicloud_agent_rs::ktp_transport::{
    KtpCryptoDirection, KtpCryptoKey, KtpCryptoRecordCodec, KtpCryptoSeal, KtpStreamCodec,
    KTP_CRYPTO_HEADER_LEN,
};
use std::error::Error;
use std::time::{Duration, Instant};

type BenchResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Copy, Debug)]
struct BenchConfig {
    mode: BenchMode,
    frames: usize,
    payload_bytes: usize,
    chunk_frames: usize,
    runs: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BenchMode {
    Stream,
    Crypto,
}

impl BenchMode {
    fn parse(value: &str) -> BenchResult<Self> {
        match value {
            "stream" => Ok(Self::Stream),
            "crypto" => Ok(Self::Crypto),
            _ => Err("--mode must be stream or crypto".into()),
        }
    }

    fn report_value(self) -> &'static str {
        match self {
            Self::Stream => "stream",
            Self::Crypto => "crypto",
        }
    }
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

    match run_benchmark(config) {
        Ok(report) => println!("{report}"),
        Err(error) => {
            eprintln!("ktp-codec-bench failed: {error}");
            std::process::exit(1);
        }
    }
}

fn parse_args(args: impl Iterator<Item = String>) -> BenchResult<BenchConfig> {
    let mut mode = BenchMode::Stream;
    let mut frames = 4096usize;
    let mut payload_bytes = 16 * 1024usize;
    let mut chunk_frames = 64usize;
    let mut runs = 1usize;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mode" => mode = BenchMode::parse(&next_value(&mut args, "--mode")?)?,
            "--frames" => {
                frames = parse_positive_usize(next_value(&mut args, "--frames")?, "--frames")?
            }
            "--payload-bytes" => {
                payload_bytes = parse_positive_usize(
                    next_value(&mut args, "--payload-bytes")?,
                    "--payload-bytes",
                )?
            }
            "--chunk-frames" => {
                chunk_frames = parse_positive_usize(
                    next_value(&mut args, "--chunk-frames")?,
                    "--chunk-frames",
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
        mode,
        frames,
        payload_bytes,
        chunk_frames,
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

fn run_benchmark(config: BenchConfig) -> BenchResult<String> {
    let chunks = build_chunks(config)?;
    let mut samples = Vec::with_capacity(config.runs);
    for _ in 0..config.runs {
        samples.push(run_decode_once(config, &chunks)?);
    }
    Ok(format_report(config, &samples))
}

fn build_chunks(config: BenchConfig) -> BenchResult<Vec<Vec<u8>>> {
    let frame = KtpFrame {
        frame_type: FrameType::SessionData,
        leg: FrameLeg::Ingress,
        flags: 0,
        session_id: 1,
        payload: vec![0x5a; config.payload_bytes],
    };

    match config.mode {
        BenchMode::Stream => build_stream_chunks(&frame, config.frames, config.chunk_frames),
        BenchMode::Crypto => build_crypto_chunks(&frame, config.frames, config.chunk_frames),
    }
}

fn build_stream_chunks(
    frame: &KtpFrame,
    frame_count: usize,
    chunk_frames: usize,
) -> BenchResult<Vec<Vec<u8>>> {
    let encoded = encode_frame(frame)?;
    let mut chunks = Vec::with_capacity(frame_count.div_ceil(chunk_frames));
    let mut remaining = frame_count;
    while remaining > 0 {
        let frames_in_chunk = remaining.min(chunk_frames);
        let mut chunk = Vec::with_capacity(encoded.len() * frames_in_chunk);
        for _ in 0..frames_in_chunk {
            chunk.extend_from_slice(&encoded);
        }
        chunks.push(chunk);
        remaining -= frames_in_chunk;
    }
    Ok(chunks)
}

fn build_crypto_chunks(
    frame: &KtpFrame,
    frame_count: usize,
    chunk_frames: usize,
) -> BenchResult<Vec<Vec<u8>>> {
    let key = KtpCryptoKey::from_bytes([0x42; 32]);
    let mut seal = KtpCryptoSeal::new(key, KtpCryptoDirection::ClientToRelay);
    let record_capacity = KTP_CRYPTO_HEADER_LEN + KTP_HEADER_LEN + frame.payload.len() + 16;
    let mut chunks = Vec::with_capacity(frame_count.div_ceil(chunk_frames));
    let mut remaining = frame_count;
    while remaining > 0 {
        let frames_in_chunk = remaining.min(chunk_frames);
        let mut chunk = Vec::with_capacity(record_capacity * frames_in_chunk);
        for _ in 0..frames_in_chunk {
            seal.append_sealed_frame(frame, &mut chunk)?;
        }
        chunks.push(chunk);
        remaining -= frames_in_chunk;
    }
    Ok(chunks)
}

fn run_decode_once(config: BenchConfig, chunks: &[Vec<u8>]) -> BenchResult<BenchSample> {
    let started = Instant::now();
    let (frames, bytes) = match config.mode {
        BenchMode::Stream => decode_stream_chunks(chunks)?,
        BenchMode::Crypto => decode_crypto_chunks(chunks)?,
    };
    let elapsed = started.elapsed();
    if frames != config.frames {
        return Err(format!("expected {} frames, decoded {frames}", config.frames).into());
    }
    Ok(BenchSample { bytes, elapsed })
}

fn decode_stream_chunks(chunks: &[Vec<u8>]) -> BenchResult<(usize, usize)> {
    let mut codec = KtpStreamCodec::new(KTP_MAX_PAYLOAD_LEN, 256 * 1024 * 1024);
    let mut frames = 0usize;
    let mut bytes = 0usize;
    for chunk in chunks {
        codec.push(chunk)?;
        while let Some(frame) = codec.next_frame()? {
            frames += 1;
            bytes += frame.payload.len();
        }
    }
    Ok((frames, bytes))
}

fn decode_crypto_chunks(chunks: &[Vec<u8>]) -> BenchResult<(usize, usize)> {
    let key = KtpCryptoKey::from_bytes([0x42; 32]);
    let mut codec = KtpCryptoRecordCodec::new(
        key,
        KtpCryptoDirection::ClientToRelay,
        KTP_MAX_PAYLOAD_LEN,
        256 * 1024 * 1024,
    );
    let mut frames = 0usize;
    let mut bytes = 0usize;
    for chunk in chunks {
        codec.push(chunk)?;
        while let Some(frame) = codec.next_frame()? {
            frames += 1;
            bytes += frame.payload.len();
        }
    }
    Ok((frames, bytes))
}

fn format_report(config: BenchConfig, samples: &[BenchSample]) -> String {
    let bytes_per_run = config.frames * config.payload_bytes;
    let total_bytes = samples.iter().map(|sample| sample.bytes).sum::<usize>();
    if samples.len() == 1 {
        let sample = samples.first().expect("single run should have a sample");
        let elapsed_secs = sample.elapsed.as_secs_f64().max(0.000_001);
        let throughput_mib_s = (sample.bytes as f64 / (1024.0 * 1024.0)) / elapsed_secs;
        return format!(
            "ktp_codec_bench mode={} runs={} frames={} payload_bytes={} chunk_frames={} bytes={} bytes_per_run={} total_bytes={} cursor_compaction=1 elapsed_ms={:.3} throughput_mib_s={:.3}",
            config.mode.report_value(),
            config.runs,
            config.frames,
            config.payload_bytes,
            config.chunk_frames,
            sample.bytes,
            bytes_per_run,
            total_bytes,
            sample.elapsed.as_secs_f64() * 1000.0,
            throughput_mib_s
        );
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

    format!(
        "ktp_codec_bench mode={} runs={} frames={} payload_bytes={} chunk_frames={} bytes={} bytes_per_run={} total_bytes={} cursor_compaction=1 elapsed_ms_min={:.3} elapsed_ms_median={:.3} elapsed_ms_max={:.3} throughput_mib_s_min={:.3} throughput_mib_s_median={:.3} throughput_mib_s_max={:.3}",
        config.mode.report_value(),
        config.runs,
        config.frames,
        config.payload_bytes,
        config.chunk_frames,
        bytes_per_run,
        bytes_per_run,
        total_bytes,
        elapsed_values[0],
        median(&elapsed_values),
        elapsed_values[elapsed_values.len() - 1],
        throughput_values[0],
        median(&throughput_values),
        throughput_values[throughput_values.len() - 1],
    )
}

fn median(sorted_values: &[f64]) -> f64 {
    let middle = sorted_values.len() / 2;
    if sorted_values.len() % 2 == 0 {
        (sorted_values[middle - 1] + sorted_values[middle]) / 2.0
    } else {
        sorted_values[middle]
    }
}

fn print_usage() {
    eprintln!(
        "usage: ktp-codec-bench [--mode stream|crypto] [--frames N] [--payload-bytes BYTES] [--chunk-frames N] [--runs N]"
    );
}
