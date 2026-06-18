use kelicloud_agent_rs::ktp::{FrameLeg, FrameType, KtpFrame};
use kelicloud_agent_rs::tunnel_async_runtime::{TunnelFrameReadyNotifier, TunnelRelayBatchPolicy};
use kelicloud_agent_rs::tunnel_control::{
    SelectedTunnelRule, TunnelRuleStateSink, TUNNEL_DATA_TRANSPORT_KTP_TCP,
};
use kelicloud_agent_rs::tunnel_runtime::{
    SharedTunnelRuleState, TunnelSessionRuntime, TunnelTcpRuntime,
};
use std::error::Error;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

type BenchResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

const RELAY_BATCH_FRAMES: usize = 64;
const RDP_LIKE_PAYLOAD_PATTERN: [usize; 16] = [
    96, 128, 160, 96, 512, 128, 96, 256, 1024, 128, 96, 160, 4096, 192, 96, 8192,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BenchProfile {
    Fixed,
    RdpLike,
}

impl BenchProfile {
    fn parse(raw: &str) -> BenchResult<Self> {
        match raw {
            "fixed" => Ok(Self::Fixed),
            "rdp-like" | "rdp_like" => Ok(Self::RdpLike),
            _ => Err("--profile must be fixed or rdp-like".into()),
        }
    }

    fn report_value(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::RdpLike => "rdp_like",
        }
    }

    fn payload_len(self, max_payload_bytes: usize, frame_index: usize) -> usize {
        match self {
            Self::Fixed => max_payload_bytes,
            Self::RdpLike => {
                let pattern_payload =
                    RDP_LIKE_PAYLOAD_PATTERN[frame_index % RDP_LIKE_PAYLOAD_PATTERN.len()];
                pattern_payload.min(max_payload_bytes).max(1)
            }
        }
    }

    fn bytes_per_client(self, frames: usize, max_payload_bytes: usize) -> usize {
        (0..frames)
            .map(|frame_index| self.payload_len(max_payload_bytes, frame_index))
            .sum()
    }
}

#[derive(Clone, Copy, Debug)]
struct BenchConfig {
    profile: BenchProfile,
    runs: usize,
    clients: usize,
    frames: usize,
    payload_bytes: usize,
    diagnostics: bool,
    latency: bool,
    relay_batch_policy: TunnelRelayBatchPolicy,
    relay_batch_frames: usize,
    relay_wait_timeout: Duration,
}

impl BenchConfig {
    fn total_payload_bytes(self) -> usize {
        self.clients
            * self
                .profile
                .bytes_per_client(self.frames, self.payload_bytes)
    }

    fn effective_relay_batch_frames(self) -> usize {
        self.relay_batch_policy
            .effective_batch_frames(self.relay_batch_frames, self.clients)
    }
}

#[derive(Clone, Debug)]
struct BenchSample {
    elapsed_ms: f64,
    throughput_mib_s: f64,
    relay_stats: RelayStats,
    client_latency_micros: Vec<Vec<u64>>,
}

#[derive(Clone, Copy, Debug, Default)]
struct RelayStats {
    relay_turns: usize,
    relay_empty_turns: usize,
    relay_yield_turns: usize,
    relay_wait_turns: usize,
    ingress_frames: usize,
    egress_frames: usize,
    ingress_data_frames: usize,
    egress_data_frames: usize,
    ingress_batches: usize,
    egress_batches: usize,
    ingress_max_batch_frames: usize,
    egress_max_batch_frames: usize,
}

impl RelayStats {
    fn record_ingress_batch(&mut self, frames: usize) {
        if frames == 0 {
            return;
        }
        self.ingress_batches += 1;
        self.ingress_max_batch_frames = self.ingress_max_batch_frames.max(frames);
    }

    fn record_egress_batch(&mut self, frames: usize) {
        if frames == 0 {
            return;
        }
        self.egress_batches += 1;
        self.egress_max_batch_frames = self.egress_max_batch_frames.max(frames);
    }
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
            eprintln!("ktp-e2e-bench failed: {error}");
            std::process::exit(1);
        }
    }
}

fn parse_args(args: impl Iterator<Item = String>) -> BenchResult<BenchConfig> {
    let mut runs = 1usize;
    let mut clients = 1usize;
    let mut frames = 1024usize;
    let mut payload_bytes = 1024usize;
    let mut profile = BenchProfile::Fixed;
    let mut diagnostics = false;
    let mut latency = false;
    let mut relay_batch_policy = TunnelRelayBatchPolicy::Fixed;
    let mut relay_batch_frames = RELAY_BATCH_FRAMES;
    let mut relay_wait_timeout = Duration::ZERO;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--diagnostics" => diagnostics = true,
            "--latency" => latency = true,
            "--relay-wait-timeout-us" => {
                let micros = parse_positive_usize(
                    next_value(&mut args, "--relay-wait-timeout-us")?,
                    "--relay-wait-timeout-us",
                )?;
                relay_wait_timeout = Duration::from_micros(micros as u64);
            }
            "--relay-batch-frames" => {
                relay_batch_frames = parse_positive_usize(
                    next_value(&mut args, "--relay-batch-frames")?,
                    "--relay-batch-frames",
                )?
            }
            "--relay-batch-policy" => {
                relay_batch_policy = TunnelRelayBatchPolicy::parse_config_value(&next_value(
                    &mut args,
                    "--relay-batch-policy",
                )?)
                .ok_or("--relay-batch-policy must be fixed or adaptive")?
            }
            "--runs" => runs = parse_positive_usize(next_value(&mut args, "--runs")?, "--runs")?,
            "--clients" => {
                clients = parse_positive_usize(next_value(&mut args, "--clients")?, "--clients")?
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
            "--profile" => profile = BenchProfile::parse(&next_value(&mut args, "--profile")?)?,
            "--help" | "-h" => return Err("help requested".into()),
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    Ok(BenchConfig {
        profile,
        runs,
        clients,
        frames,
        payload_bytes,
        diagnostics,
        latency,
        relay_batch_policy,
        relay_batch_frames,
        relay_wait_timeout,
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
    let mut samples = Vec::with_capacity(config.runs);
    for _ in 0..config.runs {
        samples.push(run_benchmark_once(config)?);
    }
    Ok(format_report(config, &samples))
}

fn run_benchmark_once(config: BenchConfig) -> BenchResult<BenchSample> {
    let target = TcpListener::bind("127.0.0.1:0")?;
    let target_addr = target.local_addr()?;
    let frames = config.frames;
    let payload_bytes = config.payload_bytes;
    let profile = config.profile;
    let clients = config.clients;
    let echo_thread = thread::spawn(move || -> std::io::Result<()> {
        let mut handles = Vec::with_capacity(clients);
        for _ in 0..clients {
            let (mut stream, _) = target.accept()?;
            let handle = thread::spawn(move || -> std::io::Result<()> {
                let mut buffer = vec![0u8; payload_bytes];
                for frame_index in 0..frames {
                    let payload_len = profile.payload_len(payload_bytes, frame_index);
                    stream.read_exact(&mut buffer[..payload_len])?;
                    stream.write_all(&buffer[..payload_len])?;
                }
                Ok(())
            });
            handles.push(handle);
        }
        for handle in handles {
            handle.join().map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::Other, "echo thread panicked")
            })??;
        }
        Ok(())
    });

    let listen_port = free_tcp_port()?;
    let frame_ready_notifier =
        (!config.relay_wait_timeout.is_zero()).then(|| Arc::new(TunnelFrameReadyNotifier::new()));
    let ingress_state = SharedTunnelRuleState::new();
    let mut ingress_rule = selected_rule(31, "ingress");
    ingress_rule.listen_port = listen_port;
    ingress_state.update_rules("bench", &[ingress_rule]);
    let mut ingress_runtime =
        new_bench_runtime(ingress_state, frame_ready_notifier.as_ref().map(Arc::clone));
    ingress_runtime.refresh_listeners()?;

    let egress_state = SharedTunnelRuleState::new();
    let mut egress_rule = selected_rule(31, "egress");
    egress_rule.target_host = "127.0.0.1".to_string();
    egress_rule.target_port = target_addr.port();
    egress_state.update_rules("bench", &[egress_rule]);
    let mut egress_runtime =
        new_bench_runtime(egress_state, frame_ready_notifier.as_ref().map(Arc::clone));

    let client_threads = (0..config.clients)
        .map(|_| {
            let collect_latency = config.latency;
            thread::spawn(move || -> std::io::Result<Vec<u64>> {
                let mut stream = connect_with_retry(("127.0.0.1", listen_port))?;
                let payload = vec![0x5a; payload_bytes];
                let mut response = vec![0u8; payload_bytes];
                let mut latency_micros = if collect_latency {
                    Vec::with_capacity(frames)
                } else {
                    Vec::new()
                };
                for frame_index in 0..frames {
                    let payload_len = config.profile.payload_len(payload_bytes, frame_index);
                    let round_started = collect_latency.then(Instant::now);
                    stream.write_all(&payload[..payload_len])?;
                    stream.read_exact(&mut response[..payload_len])?;
                    if let Some(round_started) = round_started {
                        latency_micros.push(round_started.elapsed().as_micros() as u64);
                    }
                }
                Ok(latency_micros)
            })
        })
        .collect::<Vec<_>>();

    let bytes = config.total_payload_bytes();
    let started = Instant::now();
    let relay_stats = relay_data_batches(
        &mut ingress_runtime,
        &mut egress_runtime,
        bytes,
        config.effective_relay_batch_frames(),
        config.relay_wait_timeout,
        frame_ready_notifier,
    )?;
    let mut client_latency_micros = Vec::with_capacity(config.clients);
    for client_thread in client_threads {
        let client_latencies = client_thread
            .join()
            .map_err(|_| "client thread panicked")??;
        client_latency_micros.push(client_latencies);
    }
    echo_thread.join().map_err(|_| "echo thread panicked")??;
    let elapsed = started.elapsed();
    let elapsed_secs = elapsed.as_secs_f64().max(0.000_001);
    let throughput_mib_s = (bytes as f64 / (1024.0 * 1024.0)) / elapsed_secs;

    Ok(BenchSample {
        elapsed_ms: elapsed.as_secs_f64() * 1000.0,
        throughput_mib_s,
        relay_stats,
        client_latency_micros,
    })
}

fn format_report(config: BenchConfig, samples: &[BenchSample]) -> String {
    let bytes = config.total_payload_bytes();
    if samples.len() == 1 {
        let sample = &samples[0];
        return format!(
            "ktp_e2e_bench mode=runtime_ingress_egress transport=ktp_tcp bridge=batch profile={} runs={} clients={} frames={} payload_bytes={} client_payload_reused=1 bytes={} elapsed_ms={:.3} throughput_mib_s={:.3}{}",
            config.profile.report_value(),
            config.runs,
            config.clients,
            config.frames,
            config.payload_bytes,
            bytes,
            sample.elapsed_ms,
            sample.throughput_mib_s,
            latency_suffix(config, samples) + &diagnostics_suffix(config, samples)
        );
    }

    let mut elapsed_values = samples
        .iter()
        .map(|sample| sample.elapsed_ms)
        .collect::<Vec<_>>();
    elapsed_values.sort_by(f64::total_cmp);
    let mut throughput_values = samples
        .iter()
        .map(|sample| sample.throughput_mib_s)
        .collect::<Vec<_>>();
    throughput_values.sort_by(f64::total_cmp);

    format!(
        "ktp_e2e_bench mode=runtime_ingress_egress transport=ktp_tcp bridge=batch profile={} runs={} clients={} frames={} payload_bytes={} client_payload_reused=1 bytes={} elapsed_ms_min={:.3} elapsed_ms_median={:.3} elapsed_ms_max={:.3} throughput_mib_s_min={:.3} throughput_mib_s_median={:.3} throughput_mib_s_max={:.3}{}",
        config.profile.report_value(),
        config.runs,
        config.clients,
        config.frames,
        config.payload_bytes,
        bytes,
        elapsed_values[0],
        median(&elapsed_values),
        elapsed_values[elapsed_values.len() - 1],
        throughput_values[0],
        median(&throughput_values),
        throughput_values[throughput_values.len() - 1],
        latency_suffix(config, samples) + &diagnostics_suffix(config, samples)
    )
}

fn latency_suffix(config: BenchConfig, samples: &[BenchSample]) -> String {
    if !config.latency {
        return String::new();
    }
    let mut values = samples
        .iter()
        .flat_map(|sample| {
            sample
                .client_latency_micros
                .iter()
                .flat_map(|client_samples| client_samples.iter().copied())
        })
        .collect::<Vec<_>>();
    let Some(stats) = LatencyStats::from_samples(&mut values) else {
        return " rtt_micros_samples=0".to_string();
    };
    let Some(fairness) = ClientLatencyFairness::from_samples(config.clients, samples) else {
        return format!(
            " rtt_micros_samples={} rtt_micros_p50={} rtt_micros_p95={} rtt_micros_p99={} rtt_micros_max={}",
            values.len(),
            stats.p50,
            stats.p95,
            stats.p99,
            stats.max
        );
    };
    format!(
        " rtt_micros_samples={} rtt_micros_p50={} rtt_micros_p95={} rtt_micros_p99={} rtt_micros_max={} rtt_client_p95_micros_min={} rtt_client_p95_micros_max={} rtt_client_p95_spread_micros={} rtt_client_max_micros_max={}",
        values.len(),
        stats.p50,
        stats.p95,
        stats.p99,
        stats.max,
        fairness.p95_min,
        fairness.p95_max,
        fairness.p95_spread,
        fairness.max_max
    )
}

fn diagnostics_suffix(config: BenchConfig, samples: &[BenchSample]) -> String {
    if !config.diagnostics {
        return String::new();
    }
    let mut total = RelayStats::default();
    for sample in samples {
        total.relay_turns += sample.relay_stats.relay_turns;
        total.relay_empty_turns += sample.relay_stats.relay_empty_turns;
        total.relay_yield_turns += sample.relay_stats.relay_yield_turns;
        total.relay_wait_turns += sample.relay_stats.relay_wait_turns;
        total.ingress_frames += sample.relay_stats.ingress_frames;
        total.egress_frames += sample.relay_stats.egress_frames;
        total.ingress_data_frames += sample.relay_stats.ingress_data_frames;
        total.egress_data_frames += sample.relay_stats.egress_data_frames;
        total.ingress_batches += sample.relay_stats.ingress_batches;
        total.egress_batches += sample.relay_stats.egress_batches;
        total.ingress_max_batch_frames = total
            .ingress_max_batch_frames
            .max(sample.relay_stats.ingress_max_batch_frames);
        total.egress_max_batch_frames = total
            .egress_max_batch_frames
            .max(sample.relay_stats.egress_max_batch_frames);
    }
    format!(
        " relay_batch_policy={} relay_batch_frames={} relay_batch_frames_effective={} relay_turns={} relay_empty_turns={} relay_yield_turns={} relay_wait_turns={} ingress_frames={} egress_frames={} ingress_data_frames={} egress_data_frames={} ingress_batches={} egress_batches={} ingress_max_batch_frames={} egress_max_batch_frames={}",
        config.relay_batch_policy.config_value(),
        config.relay_batch_frames,
        config.effective_relay_batch_frames(),
        total.relay_turns,
        total.relay_empty_turns,
        total.relay_yield_turns,
        total.relay_wait_turns,
        total.ingress_frames,
        total.egress_frames,
        total.ingress_data_frames,
        total.egress_data_frames,
        total.ingress_batches,
        total.egress_batches,
        total.ingress_max_batch_frames,
        total.egress_max_batch_frames
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LatencyStats {
    p50: u64,
    p95: u64,
    p99: u64,
    max: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ClientLatencyFairness {
    p95_min: u64,
    p95_max: u64,
    p95_spread: u64,
    max_max: u64,
}

impl ClientLatencyFairness {
    fn from_samples(clients: usize, samples: &[BenchSample]) -> Option<Self> {
        if clients == 0 {
            return None;
        }
        let mut by_client = vec![Vec::<u64>::new(); clients];
        for sample in samples {
            for (index, client_samples) in sample.client_latency_micros.iter().enumerate() {
                if let Some(target) = by_client.get_mut(index) {
                    target.extend(client_samples.iter().copied());
                }
            }
        }

        let mut p95_values = Vec::with_capacity(clients);
        let mut max_values = Vec::with_capacity(clients);
        for client_samples in &mut by_client {
            let stats = LatencyStats::from_samples(client_samples)?;
            p95_values.push(stats.p95);
            max_values.push(stats.max);
        }
        let p95_min = *p95_values.iter().min()?;
        let p95_max = *p95_values.iter().max()?;
        let max_max = *max_values.iter().max()?;
        Some(Self {
            p95_min,
            p95_max,
            p95_spread: p95_max.saturating_sub(p95_min),
            max_max,
        })
    }
}

impl LatencyStats {
    fn from_samples(values: &mut [u64]) -> Option<Self> {
        if values.is_empty() {
            return None;
        }
        values.sort_unstable();
        Some(Self {
            p50: percentile_nearest_rank(values, 50),
            p95: percentile_nearest_rank(values, 95),
            p99: percentile_nearest_rank(values, 99),
            max: *values.last().expect("non-empty latency values"),
        })
    }
}

fn percentile_nearest_rank(sorted_values: &[u64], percentile: usize) -> u64 {
    let rank = (sorted_values.len() * percentile).div_ceil(100);
    let index = rank.saturating_sub(1).min(sorted_values.len() - 1);
    sorted_values[index]
}

fn new_bench_runtime(
    state: SharedTunnelRuleState,
    frame_ready_notifier: Option<Arc<TunnelFrameReadyNotifier>>,
) -> TunnelTcpRuntime {
    match frame_ready_notifier {
        Some(notifier) => TunnelTcpRuntime::new_with_frame_ready_notifier_for_data_transport(
            state,
            TUNNEL_DATA_TRANSPORT_KTP_TCP,
            notifier,
        ),
        None => TunnelTcpRuntime::new_for_data_transport(state, TUNNEL_DATA_TRANSPORT_KTP_TCP),
    }
}

fn relay_data_batches(
    ingress_runtime: &mut TunnelTcpRuntime,
    egress_runtime: &mut TunnelTcpRuntime,
    expected_bytes: usize,
    relay_batch_frames: usize,
    relay_wait_timeout: Duration,
    frame_ready_notifier: Option<Arc<TunnelFrameReadyNotifier>>,
) -> BenchResult<RelayStats> {
    let mut ingress_bytes = 0usize;
    let mut egress_bytes = 0usize;
    let mut stats = RelayStats::default();
    let deadline = Instant::now() + Duration::from_secs(30);
    while egress_bytes < expected_bytes {
        stats.relay_turns += 1;
        let mut frames_this_turn = 0usize;
        let observed_generation = frame_ready_notifier
            .as_ref()
            .filter(|_| !relay_wait_timeout.is_zero())
            .map(|notifier| notifier.generation());
        let mut ingress_frames = ingress_runtime.next_client_frames(relay_batch_frames)?;
        let mut egress_frames = egress_runtime.next_client_frames(relay_batch_frames)?;
        if ingress_frames.is_empty() && egress_frames.is_empty() && !relay_wait_timeout.is_zero() {
            stats.relay_wait_turns += 1;
            if let (Some(notifier), Some(observed_generation)) =
                (frame_ready_notifier.as_ref(), observed_generation)
            {
                let _ = notifier.wait_for_change(observed_generation, relay_wait_timeout);
                ingress_frames = ingress_runtime.next_client_frames(relay_batch_frames)?;
                egress_frames = egress_runtime.next_client_frames(relay_batch_frames)?;
            } else {
                ingress_frames = ingress_runtime
                    .next_client_frames_after_wait(relay_batch_frames, relay_wait_timeout)?;
                if ingress_frames.is_empty() {
                    egress_frames = egress_runtime
                        .next_client_frames_after_wait(relay_batch_frames, relay_wait_timeout)?;
                }
            }
        }

        stats.record_ingress_batch(ingress_frames.len());
        stats.record_egress_batch(egress_frames.len());

        for frame in ingress_frames {
            stats.ingress_frames += 1;
            frames_this_turn += 1;
            if frame.frame_type == FrameType::SessionData {
                stats.ingress_data_frames += 1;
                ingress_bytes += frame.payload.len();
            }
            let responses = egress_runtime.handle_server_frame(to_leg(frame, FrameLeg::Egress))?;
            for response in responses {
                ingress_runtime.handle_server_frame(to_leg(response, FrameLeg::Ingress))?;
            }
        }
        for frame in egress_frames {
            stats.egress_frames += 1;
            frames_this_turn += 1;
            if frame.frame_type == FrameType::SessionData {
                stats.egress_data_frames += 1;
                egress_bytes += frame.payload.len();
            }
            let responses =
                ingress_runtime.handle_server_frame(to_leg(frame, FrameLeg::Ingress))?;
            for response in responses {
                egress_runtime.handle_server_frame(to_leg(response, FrameLeg::Egress))?;
            }
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "timed out relaying data: ingress_bytes={ingress_bytes}, egress_bytes={egress_bytes}, expected_bytes={expected_bytes}"
            )
            .into());
        }
        if frames_this_turn == 0 {
            stats.relay_empty_turns += 1;
        }
        if ingress_bytes < expected_bytes || egress_bytes < expected_bytes {
            stats.relay_yield_turns += 1;
            thread::yield_now();
        }
    }
    Ok(stats)
}

fn to_leg(mut frame: KtpFrame, leg: FrameLeg) -> KtpFrame {
    frame.leg = leg;
    frame
}

fn free_tcp_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

fn connect_with_retry(addr: (&str, u16)) -> std::io::Result<TcpStream> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match TcpStream::connect(addr) {
            Ok(stream) => return Ok(stream),
            Err(error) if Instant::now() < deadline => {
                let _ = error;
                thread::sleep(Duration::from_millis(1));
            }
            Err(error) => return Err(error),
        }
    }
}

fn selected_rule(id: u64, role: &str) -> SelectedTunnelRule {
    SelectedTunnelRule {
        id,
        name: format!("bench-rule-{id}"),
        enabled: true,
        protocol: "tcp".to_string(),
        role: role.to_string(),
        ingress_group: "edge".to_string(),
        listen_address: "127.0.0.1".to_string(),
        listen_port: 10000 + id as u16,
        egress_group: "rdp".to_string(),
        target_host: "127.0.0.1".to_string(),
        target_port: 3389,
        source_allowlist: "127.0.0.0/8".to_string(),
        max_concurrent_sessions: 32,
        last_revision: 1,
        data_transport: TUNNEL_DATA_TRANSPORT_KTP_TCP.to_string(),
    }
}

fn print_usage() {
    eprintln!("usage: ktp-e2e-bench [--diagnostics] [--latency] [--profile fixed|rdp-like] [--relay-batch-policy fixed|adaptive] [--relay-batch-frames N] [--relay-wait-timeout-us MICROS] [--runs N] [--clients N] [--frames N] [--payload-bytes BYTES]");
}
