use crate::config::AgentConfig;
use crate::ping::{NoopPingExecutor, PingExecutor, PingTask};
use crate::protocol::{build_report_ws_url, BackendMessage, ProtocolError};
use crate::report::{BasicInfo, ReportGenerator};
use crate::transport::{
    access_headers, build_basic_info_url, parse_socket_message, HeaderPair, HttpTransport,
    ReportSocket, TransportError, WebSocketTransport,
};
use std::error::Error;
use std::fmt;
use std::thread;
use std::time::Duration;

#[derive(Debug)]
pub enum RuntimeError {
    Protocol(ProtocolError),
    Transport(TransportError),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(error) => write!(f, "{error}"),
            Self::Transport(error) => write!(f, "{error}"),
        }
    }
}

impl Error for RuntimeError {}

impl From<ProtocolError> for RuntimeError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}

impl From<TransportError> for RuntimeError {
    fn from(value: TransportError) -> Self {
        Self::Transport(value)
    }
}

pub trait ControlMessageHandler {
    fn handle(&mut self, message: BackendMessage);
}

#[derive(Debug, Default)]
pub struct NoopControlMessageHandler;

impl ControlMessageHandler for NoopControlMessageHandler {
    fn handle(&mut self, _message: BackendMessage) {}
}

pub trait LoopDelay {
    fn sleep_report_interval(&mut self, seconds: f64);
    fn sleep_reconnect_interval(&mut self, seconds: u64);
}

#[derive(Debug, Default)]
pub struct NoopLoopDelay;

impl LoopDelay for NoopLoopDelay {
    fn sleep_report_interval(&mut self, _seconds: f64) {}
    fn sleep_reconnect_interval(&mut self, _seconds: u64) {}
}

#[derive(Debug, Default)]
pub struct ThreadLoopDelay;

impl LoopDelay for ThreadLoopDelay {
    fn sleep_report_interval(&mut self, seconds: f64) {
        if seconds.is_finite() && seconds > 0.0 {
            thread::sleep(Duration::from_secs_f64(seconds));
        }
    }

    fn sleep_reconnect_interval(&mut self, seconds: u64) {
        if seconds > 0 {
            thread::sleep(Duration::from_secs(seconds));
        }
    }
}

pub fn run_once<H, W, G, C>(
    config: &AgentConfig,
    basic_info: &BasicInfo,
    report_generator: &G,
    http: &mut H,
    websocket: &mut W,
    handler: &mut C,
) -> Result<(), RuntimeError>
where
    H: HttpTransport,
    W: WebSocketTransport,
    G: ReportGenerator,
    C: ControlMessageHandler,
{
    let headers = access_headers(config);
    let basic_info_url = build_basic_info_url(&config.endpoint, &config.token)?;
    upload_basic_info_with_legacy_retry(http, &basic_info_url, &headers, basic_info)?;

    let report_url = build_report_ws_url(&config.endpoint, &config.token)?;
    let mut socket = websocket.connect_report(&report_url, &headers)?;
    let report = report_generator.generate();
    socket.send_report(&report)?;

    if let Some(bytes) = socket.read_message()? {
        handler.handle(parse_socket_message(&bytes));
    }

    Ok(())
}

pub fn run_once_with_ping<H, W, G, P, C>(
    config: &AgentConfig,
    basic_info: &BasicInfo,
    report_generator: &G,
    ping_executor: &P,
    http: &mut H,
    websocket: &mut W,
    handler: &mut C,
) -> Result<(), RuntimeError>
where
    H: HttpTransport,
    W: WebSocketTransport,
    G: ReportGenerator,
    P: PingExecutor,
    C: ControlMessageHandler,
{
    let headers = access_headers(config);
    let basic_info_url = build_basic_info_url(&config.endpoint, &config.token)?;
    upload_basic_info_with_legacy_retry(http, &basic_info_url, &headers, basic_info)?;

    let report_url = build_report_ws_url(&config.endpoint, &config.token)?;
    let mut socket = websocket.connect_report(&report_url, &headers)?;
    let report = report_generator.generate();
    socket.send_report(&report)?;

    if let Some(bytes) = socket.read_message()? {
        process_backend_message(&mut socket, handler, ping_executor, &bytes)?;
    }

    Ok(())
}

pub fn run_report_cycles<H, W, G, C>(
    config: &AgentConfig,
    basic_info: &BasicInfo,
    report_generator: &G,
    http: &mut H,
    websocket: &mut W,
    handler: &mut C,
    cycles: usize,
) -> Result<(), RuntimeError>
where
    H: HttpTransport,
    W: WebSocketTransport,
    G: ReportGenerator,
    C: ControlMessageHandler,
{
    let mut delay = NoopLoopDelay;
    run_report_cycles_with_delay(
        config,
        basic_info,
        report_generator,
        http,
        websocket,
        handler,
        &mut delay,
        cycles,
    )
}

pub fn run_report_cycles_with_delay<H, W, G, C, D>(
    config: &AgentConfig,
    basic_info: &BasicInfo,
    report_generator: &G,
    http: &mut H,
    websocket: &mut W,
    handler: &mut C,
    delay: &mut D,
    cycles: usize,
) -> Result<(), RuntimeError>
where
    H: HttpTransport,
    W: WebSocketTransport,
    G: ReportGenerator,
    C: ControlMessageHandler,
    D: LoopDelay,
{
    let ping_executor = NoopPingExecutor;
    run_report_cycles_with_ping_delay(
        config,
        basic_info,
        report_generator,
        &ping_executor,
        http,
        websocket,
        handler,
        delay,
        cycles,
    )
}

pub fn run_report_cycles_with_ping_delay<H, W, G, P, C, D>(
    config: &AgentConfig,
    basic_info: &BasicInfo,
    report_generator: &G,
    ping_executor: &P,
    http: &mut H,
    websocket: &mut W,
    handler: &mut C,
    delay: &mut D,
    cycles: usize,
) -> Result<(), RuntimeError>
where
    H: HttpTransport,
    W: WebSocketTransport,
    G: ReportGenerator,
    P: PingExecutor,
    C: ControlMessageHandler,
    D: LoopDelay,
{
    let headers = access_headers(config);
    let basic_info_url = build_basic_info_url(&config.endpoint, &config.token)?;
    upload_basic_info_with_legacy_retry(http, &basic_info_url, &headers, basic_info)?;

    let report_url = build_report_ws_url(&config.endpoint, &config.token)?;
    let heartbeat_every = heartbeat_interval_cycles(config.interval_seconds);
    let mut socket: Option<W::Socket> = None;
    let mut connect_failures = 0_u32;

    for cycle in 0..cycles {
        if socket.is_none() {
            match websocket.connect_report(&report_url, &headers) {
                Ok(next_socket) => {
                    socket = Some(next_socket);
                    connect_failures = 0;
                }
                Err(error) => {
                    connect_failures += 1;
                    if connect_failures > config.max_retries {
                        return Err(RuntimeError::Transport(error));
                    }
                    delay.sleep_reconnect_interval(config.reconnect_interval_seconds);
                    continue;
                }
            }
        }

        let mut disconnect = false;
        if let Some(active_socket) = socket.as_mut() {
            let report = report_generator.generate();
            if active_socket.send_report(&report).is_err() {
                disconnect = true;
            } else {
                match active_socket.read_message() {
                    Ok(Some(bytes)) => {
                        process_backend_message(active_socket, handler, ping_executor, &bytes)?
                    }
                    Ok(None) => {}
                    Err(_) => disconnect = true,
                }
                if !disconnect && (cycle + 1) % heartbeat_every == 0 {
                    if active_socket.send_ping().is_err() {
                        disconnect = true;
                    }
                }
            }
        }

        if disconnect {
            socket = None;
        }

        if cycle + 1 < cycles {
            delay.sleep_report_interval(config.interval_seconds);
        }
    }

    Ok(())
}

pub fn upload_basic_info_with_legacy_retry<H>(
    http: &mut H,
    url: &str,
    headers: &[HeaderPair],
    basic_info: &BasicInfo,
) -> Result<(), TransportError>
where
    H: HttpTransport,
{
    match http.upload_basic_info(url, headers, basic_info) {
        Ok(()) => Ok(()),
        Err(error) => {
            if basic_info.kernel_version.is_empty() {
                return Err(error);
            }
            http.upload_basic_info(url, headers, &basic_info.without_kernel_version())
        }
    }
}

fn process_backend_message<S, C, P>(
    socket: &mut S,
    handler: &mut C,
    ping_executor: &P,
    bytes: &[u8],
) -> Result<(), RuntimeError>
where
    S: ReportSocket,
    C: ControlMessageHandler,
    P: PingExecutor,
{
    let message = parse_socket_message(bytes);
    match message {
        BackendMessage::Ping {
            task_id,
            ping_type,
            target,
        } => {
            let task = PingTask {
                task_id,
                ping_type,
                target,
            };
            let result = ping_executor.run(&task);
            socket.send_ping_result(&result)?;
        }
        other => handler.handle(other),
    }

    Ok(())
}

fn heartbeat_interval_cycles(interval_seconds: f64) -> usize {
    if interval_seconds <= 0.0 {
        return 30;
    }

    (30.0 / interval_seconds).ceil().max(1.0) as usize
}

pub fn startup_summary(config: &AgentConfig) -> String {
    let tls = if config.insecure {
        "insecure tls: enabled"
    } else {
        "insecure tls: disabled"
    };
    let web_ssh = if config.disable_web_ssh {
        "web ssh: disabled"
    } else {
        "web ssh: enabled"
    };

    format!(
        "kelicloud-agent-rs prototype\nendpoint: {}\ntoken: {}\n{tls}\n{web_ssh}",
        config.endpoint,
        redact_secret(&config.token),
    )
}

fn redact_secret(secret: &str) -> String {
    let chars = secret.chars().collect::<Vec<_>>();
    if chars.len() <= 8 {
        return "****".to_string();
    }

    let prefix = chars.iter().take(4).collect::<String>();
    let suffix = chars
        .iter()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("{prefix}...{suffix}")
}
