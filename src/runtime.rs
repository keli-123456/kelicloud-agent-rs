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

const CONTROL_MESSAGE_POLL_INTERVAL_SECONDS: f64 = 1.0;

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

#[derive(Debug)]
pub struct ChainControlMessageHandler<A, B> {
    first: A,
    second: B,
}

impl<A, B> ChainControlMessageHandler<A, B> {
    pub fn new(first: A, second: B) -> Self {
        Self { first, second }
    }

    pub fn first(&self) -> &A {
        &self.first
    }

    pub fn second(&self) -> &B {
        &self.second
    }
}

impl<A, B> ControlMessageHandler for ChainControlMessageHandler<A, B>
where
    A: ControlMessageHandler,
    B: ControlMessageHandler,
{
    fn handle(&mut self, message: BackendMessage) {
        self.first.handle(message.clone());
        self.second.handle(message);
    }
}

pub trait BasicInfoProvider {
    fn basic_info(&self) -> BasicInfo;
}

impl BasicInfoProvider for BasicInfo {
    fn basic_info(&self) -> BasicInfo {
        self.clone()
    }
}

impl<F> BasicInfoProvider for F
where
    F: Fn() -> BasicInfo,
{
    fn basic_info(&self) -> BasicInfo {
        self()
    }
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

pub trait TokenRecovery {
    fn recover_from_transport_error(
        &mut self,
        config: &mut AgentConfig,
        error: &TransportError,
    ) -> bool;
}

#[derive(Debug, Default)]
pub struct NoopLoopDelay;

impl LoopDelay for NoopLoopDelay {
    fn sleep_report_interval(&mut self, _seconds: f64) {}
    fn sleep_reconnect_interval(&mut self, _seconds: u64) {}
}

#[derive(Debug, Default)]
pub struct NoopTokenRecovery;

impl TokenRecovery for NoopTokenRecovery {
    fn recover_from_transport_error(
        &mut self,
        _config: &mut AgentConfig,
        _error: &TransportError,
    ) -> bool {
        false
    }
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

pub fn run_once<H, W, G, C, B>(
    config: &AgentConfig,
    basic_info_provider: &B,
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
    B: BasicInfoProvider,
{
    let headers = access_headers(config);
    let basic_info_url = build_basic_info_url(&config.endpoint, &config.token)?;
    let basic_info = basic_info_provider.basic_info();
    upload_basic_info_with_legacy_retry(http, &basic_info_url, &headers, &basic_info)?;

    let report_url = build_report_ws_url(&config.endpoint, &config.token)?;
    let mut socket = websocket.connect_report(&report_url, &headers)?;
    let report = report_generator.generate();
    socket.send_report(&report)?;

    if let Some(bytes) = socket.read_message()? {
        handler.handle(parse_socket_message(&bytes));
    }

    Ok(())
}

pub fn run_once_with_ping<H, W, G, P, C, B>(
    config: &AgentConfig,
    basic_info_provider: &B,
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
    B: BasicInfoProvider,
{
    let mut config = config.clone();
    let mut recovery = NoopTokenRecovery;
    run_once_with_ping_and_token_recovery(
        &mut config,
        basic_info_provider,
        report_generator,
        ping_executor,
        http,
        websocket,
        handler,
        &mut recovery,
    )
}

pub fn run_once_with_ping_and_token_recovery<H, W, G, P, C, B, R>(
    config: &mut AgentConfig,
    basic_info_provider: &B,
    report_generator: &G,
    ping_executor: &P,
    http: &mut H,
    websocket: &mut W,
    handler: &mut C,
    recovery: &mut R,
) -> Result<(), RuntimeError>
where
    H: HttpTransport,
    W: WebSocketTransport,
    G: ReportGenerator,
    P: PingExecutor,
    C: ControlMessageHandler,
    B: BasicInfoProvider,
    R: TokenRecovery,
{
    let headers = access_headers(config);
    let basic_info = basic_info_provider.basic_info();
    upload_basic_info_with_token_recovery(config, http, &headers, &basic_info, recovery)?;

    let mut socket = connect_report_with_token_recovery(config, websocket, &headers, recovery)?;
    let report = report_generator.generate();
    socket.send_report(&report)?;

    drain_backend_messages(&mut socket, handler, ping_executor)?;

    Ok(())
}

fn upload_basic_info_with_token_recovery<H, R>(
    config: &mut AgentConfig,
    http: &mut H,
    headers: &[HeaderPair],
    basic_info: &BasicInfo,
    recovery: &mut R,
) -> Result<bool, RuntimeError>
where
    H: HttpTransport,
    R: TokenRecovery,
{
    let mut recovered = false;
    loop {
        let basic_info_url = build_basic_info_url(&config.endpoint, &config.token)?;
        match upload_basic_info_with_legacy_retry(http, &basic_info_url, headers, basic_info) {
            Ok(()) => return Ok(recovered),
            Err(error) if !recovered && recovery.recover_from_transport_error(config, &error) => {
                recovered = true;
            }
            Err(error) => return Err(RuntimeError::Transport(error)),
        }
    }
}

fn connect_report_with_token_recovery<W, R>(
    config: &mut AgentConfig,
    websocket: &mut W,
    headers: &[HeaderPair],
    recovery: &mut R,
) -> Result<W::Socket, RuntimeError>
where
    W: WebSocketTransport,
    R: TokenRecovery,
{
    let mut recovered = false;
    loop {
        let report_url = build_report_ws_url(&config.endpoint, &config.token)?;
        match websocket.connect_report(&report_url, headers) {
            Ok(socket) => return Ok(socket),
            Err(error) if !recovered && recovery.recover_from_transport_error(config, &error) => {
                recovered = true;
            }
            Err(error) => return Err(RuntimeError::Transport(error)),
        }
    }
}

pub fn run_report_cycles<H, W, G, C, B>(
    config: &AgentConfig,
    basic_info_provider: &B,
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
    B: BasicInfoProvider,
{
    let mut delay = NoopLoopDelay;
    run_report_cycles_with_delay(
        config,
        basic_info_provider,
        report_generator,
        http,
        websocket,
        handler,
        &mut delay,
        cycles,
    )
}

pub fn run_report_cycles_with_delay<H, W, G, C, D, B>(
    config: &AgentConfig,
    basic_info_provider: &B,
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
    B: BasicInfoProvider,
{
    let ping_executor = NoopPingExecutor;
    run_report_cycles_with_ping_delay(
        config,
        basic_info_provider,
        report_generator,
        &ping_executor,
        http,
        websocket,
        handler,
        delay,
        cycles,
    )
}

pub fn run_report_cycles_with_ping_delay<H, W, G, P, C, D, B>(
    config: &AgentConfig,
    basic_info_provider: &B,
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
    B: BasicInfoProvider,
{
    let mut config = config.clone();
    let mut recovery = NoopTokenRecovery;
    run_report_cycles_with_ping_delay_and_token_recovery(
        &mut config,
        basic_info_provider,
        report_generator,
        ping_executor,
        http,
        websocket,
        handler,
        delay,
        &mut recovery,
        cycles,
    )
}

pub fn run_report_cycles_with_ping_delay_and_token_recovery<H, W, G, P, C, D, B, R>(
    config: &mut AgentConfig,
    basic_info_provider: &B,
    report_generator: &G,
    ping_executor: &P,
    http: &mut H,
    websocket: &mut W,
    handler: &mut C,
    delay: &mut D,
    recovery: &mut R,
    cycles: usize,
) -> Result<(), RuntimeError>
where
    H: HttpTransport,
    W: WebSocketTransport,
    G: ReportGenerator,
    P: PingExecutor,
    C: ControlMessageHandler,
    D: LoopDelay,
    B: BasicInfoProvider,
    R: TokenRecovery,
{
    let headers = access_headers(config);
    let report_interval_seconds = report_tick_interval_seconds(config.interval_seconds);
    let heartbeat_every = heartbeat_interval_cycles(report_interval_seconds);
    let basic_info_every =
        basic_info_interval_cycles(report_interval_seconds, config.info_report_interval_minutes);
    let mut socket: Option<W::Socket> = None;
    let mut connect_failures = 0_u32;

    for cycle in 0..cycles {
        if cycle % basic_info_every == 0 {
            let basic_info = basic_info_provider.basic_info();
            let upload_result = upload_basic_info_with_token_recovery(
                config,
                http,
                &headers,
                &basic_info,
                recovery,
            );
            match upload_result {
                Ok(true) => socket = None,
                Ok(false) => {}
                Err(error) if cycle == 0 => return Err(error),
                Err(_) => {}
            }
        }

        if socket.is_none() {
            match connect_report_with_token_recovery(config, websocket, &headers, recovery) {
                Ok(next_socket) => {
                    socket = Some(next_socket);
                    connect_failures = 0;
                }
                Err(error) => {
                    connect_failures += 1;
                    if connect_failures > config.max_retries {
                        return Err(error);
                    }
                    delay.sleep_reconnect_interval(config.reconnect_interval_seconds);
                    continue;
                }
            }
        }

        let mut disconnect = false;
        if let Some(active_socket) = socket.as_mut() {
            if drain_backend_messages_for_cycle(active_socket, handler, ping_executor)? {
                disconnect = true;
            } else {
                let report = report_generator.generate();
                if active_socket.send_report(&report).is_err() {
                    disconnect = true;
                } else if drain_backend_messages_for_cycle(active_socket, handler, ping_executor)? {
                    disconnect = true;
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
            sleep_report_interval_with_control_messages(
                &mut socket,
                handler,
                ping_executor,
                delay,
                report_interval_seconds,
            )?;
        }
    }

    Ok(())
}

fn sleep_report_interval_with_control_messages<S, C, P, D>(
    socket: &mut Option<S>,
    handler: &mut C,
    ping_executor: &P,
    delay: &mut D,
    seconds: f64,
) -> Result<(), RuntimeError>
where
    S: ReportSocket,
    C: ControlMessageHandler,
    P: PingExecutor,
    D: LoopDelay,
{
    if !seconds.is_finite() || seconds <= 0.0 {
        return Ok(());
    }

    let mut remaining = seconds;
    while remaining > 0.0 {
        let sleep_for = remaining.min(CONTROL_MESSAGE_POLL_INTERVAL_SECONDS);
        delay.sleep_report_interval(sleep_for);
        remaining = (remaining - sleep_for).max(0.0);

        if let Some(active_socket) = socket.as_mut() {
            if drain_backend_messages_for_cycle(active_socket, handler, ping_executor)? {
                *socket = None;
                break;
            }
        }
    }

    Ok(())
}

fn drain_backend_messages_for_cycle<S, C, P>(
    socket: &mut S,
    handler: &mut C,
    ping_executor: &P,
) -> Result<bool, RuntimeError>
where
    S: ReportSocket,
    C: ControlMessageHandler,
    P: PingExecutor,
{
    loop {
        match socket.read_message() {
            Ok(Some(bytes)) => process_backend_message(socket, handler, ping_executor, &bytes)?,
            Ok(None) => return Ok(false),
            Err(_) => return Ok(true),
        }
    }
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
            if matches!(error, TransportError::InvalidClientToken { .. }) {
                return Err(error);
            }
            if basic_info.kernel_version.is_empty() {
                return Err(error);
            }
            http.upload_basic_info(url, headers, &basic_info.without_kernel_version())
        }
    }
}

fn drain_backend_messages<S, C, P>(
    socket: &mut S,
    handler: &mut C,
    ping_executor: &P,
) -> Result<(), RuntimeError>
where
    S: ReportSocket,
    C: ControlMessageHandler,
    P: PingExecutor,
{
    while let Some(bytes) = socket.read_message()? {
        process_backend_message(socket, handler, ping_executor, &bytes)?;
    }

    Ok(())
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

fn report_tick_interval_seconds(config_interval_seconds: f64) -> f64 {
    if !config_interval_seconds.is_finite() || config_interval_seconds <= 1.0 {
        1.0
    } else {
        config_interval_seconds - 1.0
    }
}

fn heartbeat_interval_cycles(interval_seconds: f64) -> usize {
    if !interval_seconds.is_finite() || interval_seconds <= 0.0 {
        return 30;
    }

    (30.0 / interval_seconds).ceil().max(1.0) as usize
}

fn basic_info_interval_cycles(
    report_interval_seconds: f64,
    info_report_interval_minutes: u64,
) -> usize {
    if !report_interval_seconds.is_finite()
        || report_interval_seconds <= 0.0
        || info_report_interval_minutes == 0
    {
        return 1;
    }

    ((info_report_interval_minutes as f64 * 60.0) / report_interval_seconds)
        .ceil()
        .max(1.0) as usize
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

    let mut summary = format!(
        "kelicloud-agent-rs prototype\nendpoint: {}\ntoken: {}\n{tls}\n{web_ssh}",
        config.endpoint,
        redact_secret(&config.token),
    );
    if config.tunnel_data_enabled {
        let tuning = config.tunnel_ktp_relay_batch_tuning;
        summary.push_str(&format!(
            "\ntunnel data: enabled\nktp relay batch policy: {}\nadaptive high_sessions={} elevated_dwell_us={} severe_dwell_us={} elevated_cap={} severe_cap={}",
            config.tunnel_ktp_relay_batch_policy.config_value(),
            tuning.high_session_threshold,
            tuning.elevated_dwell_p95_micros,
            tuning.severe_dwell_p95_micros,
            tuning.elevated_batch_cap,
            tuning.severe_batch_cap
        ));
    }
    summary
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
