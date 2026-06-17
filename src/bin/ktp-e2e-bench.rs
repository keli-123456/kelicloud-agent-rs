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

#[derive(Clone, Copy, Debug)]
struct BenchConfig {
    frames: usize,
    payload_bytes: usize,
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
    let mut frames = 1024usize;
    let mut payload_bytes = 1024usize;
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
            "--help" | "-h" => return Err("help requested".into()),
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    Ok(BenchConfig {
        frames,
        payload_bytes,
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
    let target = TcpListener::bind("127.0.0.1:0")?;
    let target_addr = target.local_addr()?;
    let frames = config.frames;
    let payload_bytes = config.payload_bytes;
    let echo_thread = thread::spawn(move || -> std::io::Result<()> {
        let (mut stream, _) = target.accept()?;
        let mut buffer = vec![0u8; payload_bytes];
        for _ in 0..frames {
            stream.read_exact(&mut buffer)?;
            stream.write_all(&buffer)?;
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

    let client_thread = thread::spawn(move || -> std::io::Result<()> {
        let mut stream = connect_with_retry(("127.0.0.1", listen_port))?;
        let payload = vec![0x5a; payload_bytes];
        let mut response = vec![0u8; payload_bytes];
        for _ in 0..frames {
            stream.write_all(&payload)?;
            stream.read_exact(&mut response)?;
        }
        Ok(())
    });

    let bytes = config.frames * config.payload_bytes;
    let started = Instant::now();
    relay_open(&mut ingress_runtime, &mut egress_runtime)?;
    for _ in 0..config.frames {
        relay_next_data(&mut ingress_runtime, &mut egress_runtime)?;
    }
    client_thread
        .join()
        .map_err(|_| "client thread panicked")??;
    echo_thread.join().map_err(|_| "echo thread panicked")??;
    let elapsed = started.elapsed();
    let elapsed_secs = elapsed.as_secs_f64().max(0.000_001);
    let throughput_mib_s = (bytes as f64 / (1024.0 * 1024.0)) / elapsed_secs;

    Ok(format!(
        "ktp_e2e_bench mode=runtime_ingress_egress transport=ktp_tcp frames={} payload_bytes={} bytes={} elapsed_ms={:.3} throughput_mib_s={:.3}",
        config.frames,
        config.payload_bytes,
        bytes,
        elapsed.as_secs_f64() * 1000.0,
        throughput_mib_s
    ))
}

fn relay_open(
    ingress_runtime: &mut TunnelTcpRuntime,
    egress_runtime: &mut TunnelTcpRuntime,
) -> BenchResult<()> {
    let open = wait_for_next_runtime_frame(ingress_runtime, FrameType::SessionOpen)?;
    let responses = egress_runtime.handle_server_frame(to_leg(open, FrameLeg::Egress))?;
    for response in responses {
        ingress_runtime.handle_server_frame(to_leg(response, FrameLeg::Ingress))?;
    }
    Ok(())
}

fn relay_next_data(
    ingress_runtime: &mut TunnelTcpRuntime,
    egress_runtime: &mut TunnelTcpRuntime,
) -> BenchResult<()> {
    let data = wait_for_next_runtime_frame(ingress_runtime, FrameType::SessionData)?;
    egress_runtime.handle_server_frame(to_leg(data, FrameLeg::Egress))?;
    let response = wait_for_next_runtime_frame(egress_runtime, FrameType::SessionData)?;
    ingress_runtime.handle_server_frame(to_leg(response, FrameLeg::Ingress))?;
    Ok(())
}

fn wait_for_next_runtime_frame(
    runtime: &mut TunnelTcpRuntime,
    expected_type: FrameType,
) -> BenchResult<KtpFrame> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(frame) = runtime.next_client_frame()? {
            if frame.frame_type == expected_type {
                return Ok(frame);
            }
        }
        thread::yield_now();
    }
    Err(format!("timed out waiting for {expected_type:?}").into())
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
    eprintln!("usage: ktp-e2e-bench [--frames N] [--payload-bytes BYTES]");
}
