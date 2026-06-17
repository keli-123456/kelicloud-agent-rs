use kelicloud_agent_rs::ktp::{FrameLeg, FrameType, KtpFrame};
use kelicloud_agent_rs::tunnel_control::{
    SelectedTunnelRule, TunnelRuleStateSink, TUNNEL_DATA_TRANSPORT_KTP_TCP,
};
use kelicloud_agent_rs::tunnel_runtime::{
    SharedTunnelRuleState, TunnelSessionRuntime, TunnelTcpRuntime,
};
use std::error::Error;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

type BenchResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

const RELAY_BATCH_FRAMES: usize = 64;

#[derive(Clone, Copy, Debug)]
struct BenchConfig {
    runs: usize,
    clients: usize,
    frames: usize,
    payload_bytes: usize,
    diagnostics: bool,
    relay_wait_timeout: Duration,
}

#[derive(Clone, Copy, Debug)]
struct BenchSample {
    elapsed_ms: f64,
    throughput_mib_s: f64,
    relay_stats: RelayStats,
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
    let mut diagnostics = false;
    let mut relay_wait_timeout = Duration::ZERO;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--diagnostics" => diagnostics = true,
            "--relay-wait-timeout-us" => {
                let micros = parse_positive_usize(
                    next_value(&mut args, "--relay-wait-timeout-us")?,
                    "--relay-wait-timeout-us",
                )?;
                relay_wait_timeout = Duration::from_micros(micros as u64);
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
            "--help" | "-h" => return Err("help requested".into()),
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    Ok(BenchConfig {
        runs,
        clients,
        frames,
        payload_bytes,
        diagnostics,
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
            handle.join().map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::Other, "echo thread panicked")
            })??;
        }
        Ok(())
    });

    let listen_port = free_tcp_port()?;
    let ingress_state = SharedTunnelRuleState::new();
    let mut ingress_rule = selected_rule(31, "ingress");
    ingress_rule.listen_port = listen_port;
    ingress_state.update_rules("bench", &[ingress_rule]);
    let mut ingress_runtime =
        TunnelTcpRuntime::new_for_data_transport(ingress_state, TUNNEL_DATA_TRANSPORT_KTP_TCP);
    ingress_runtime.refresh_listeners()?;

    let egress_state = SharedTunnelRuleState::new();
    let mut egress_rule = selected_rule(31, "egress");
    egress_rule.target_host = "127.0.0.1".to_string();
    egress_rule.target_port = target_addr.port();
    egress_state.update_rules("bench", &[egress_rule]);
    let mut egress_runtime =
        TunnelTcpRuntime::new_for_data_transport(egress_state, TUNNEL_DATA_TRANSPORT_KTP_TCP);

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

    let bytes = config.clients * config.frames * config.payload_bytes;
    let started = Instant::now();
    let relay_stats = relay_data_batches(
        &mut ingress_runtime,
        &mut egress_runtime,
        bytes,
        config.relay_wait_timeout,
    )?;
    for client_thread in client_threads {
        client_thread
            .join()
            .map_err(|_| "client thread panicked")??;
    }
    echo_thread.join().map_err(|_| "echo thread panicked")??;
    let elapsed = started.elapsed();
    let elapsed_secs = elapsed.as_secs_f64().max(0.000_001);
    let throughput_mib_s = (bytes as f64 / (1024.0 * 1024.0)) / elapsed_secs;

    Ok(BenchSample {
        elapsed_ms: elapsed.as_secs_f64() * 1000.0,
        throughput_mib_s,
        relay_stats,
    })
}

fn format_report(config: BenchConfig, samples: &[BenchSample]) -> String {
    let bytes = config.clients * config.frames * config.payload_bytes;
    if samples.len() == 1 {
        let sample = samples[0];
        return format!(
            "ktp_e2e_bench mode=runtime_ingress_egress transport=ktp_tcp bridge=batch runs={} clients={} frames={} payload_bytes={} bytes={} elapsed_ms={:.3} throughput_mib_s={:.3}{}",
            config.runs,
            config.clients,
            config.frames,
            config.payload_bytes,
            bytes,
            sample.elapsed_ms,
            sample.throughput_mib_s,
            diagnostics_suffix(config, samples)
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
        "ktp_e2e_bench mode=runtime_ingress_egress transport=ktp_tcp bridge=batch runs={} clients={} frames={} payload_bytes={} bytes={} elapsed_ms_min={:.3} elapsed_ms_median={:.3} elapsed_ms_max={:.3} throughput_mib_s_min={:.3} throughput_mib_s_median={:.3} throughput_mib_s_max={:.3}{}",
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
        diagnostics_suffix(config, samples)
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
    }
    format!(
        " relay_turns={} relay_empty_turns={} relay_yield_turns={} relay_wait_turns={} ingress_frames={} egress_frames={} ingress_data_frames={} egress_data_frames={}",
        total.relay_turns,
        total.relay_empty_turns,
        total.relay_yield_turns,
        total.relay_wait_turns,
        total.ingress_frames,
        total.egress_frames,
        total.ingress_data_frames,
        total.egress_data_frames
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

fn relay_data_batches(
    ingress_runtime: &mut TunnelTcpRuntime,
    egress_runtime: &mut TunnelTcpRuntime,
    expected_bytes: usize,
    relay_wait_timeout: Duration,
) -> BenchResult<RelayStats> {
    let mut ingress_bytes = 0usize;
    let mut egress_bytes = 0usize;
    let mut stats = RelayStats::default();
    let deadline = Instant::now() + Duration::from_secs(30);
    while egress_bytes < expected_bytes {
        stats.relay_turns += 1;
        let mut frames_this_turn = 0usize;
        let mut ingress_frames = ingress_runtime.next_client_frames(RELAY_BATCH_FRAMES)?;
        let mut egress_frames = egress_runtime.next_client_frames(RELAY_BATCH_FRAMES)?;
        if ingress_frames.is_empty() && egress_frames.is_empty() && !relay_wait_timeout.is_zero() {
            stats.relay_wait_turns += 1;
            ingress_frames = ingress_runtime
                .next_client_frames_after_wait(RELAY_BATCH_FRAMES, relay_wait_timeout)?;
            if ingress_frames.is_empty() {
                egress_frames = egress_runtime
                    .next_client_frames_after_wait(RELAY_BATCH_FRAMES, relay_wait_timeout)?;
            }
        }

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
    eprintln!("usage: ktp-e2e-bench [--diagnostics] [--relay-wait-timeout-us MICROS] [--runs N] [--clients N] [--frames N] [--payload-bytes BYTES]");
}
