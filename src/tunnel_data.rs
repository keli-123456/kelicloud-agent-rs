use crate::ktp::{decode_frame, encode_frame, FrameType, KtpFrame, KTP_MAX_PAYLOAD_LEN};
use crate::ktp_transport::{
    KtpCryptoDirection, KtpCryptoKey, KtpEncryptedTcpStream, KtpTcpTransportError,
};
use crate::transport::{connect_websocket_request, HeaderPair, TransportError};
use crate::tunnel_control::SelectedTunnelRule;
use crate::tunnel_runtime::{NoopTunnelSessionRuntime, TunnelSessionRuntime};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream as TokioTcpStream;
use tokio::runtime::Runtime;
use tokio::time::timeout;
use tungstenite::client::IntoClientRequest;
use tungstenite::http::{HeaderName, HeaderValue};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TunnelDataReadyState {
    pub revision: String,
    pub ingress_rule_ids: Vec<u64>,
    pub egress_rule_ids: Vec<u64>,
    pub failed_rules: Vec<TunnelDataRuleFailure>,
}

impl TunnelDataReadyState {
    pub fn empty(revision: &str) -> Self {
        Self {
            revision: revision.trim().to_string(),
            ingress_rule_ids: Vec::new(),
            egress_rule_ids: Vec::new(),
            failed_rules: Vec::new(),
        }
    }

    pub fn from_selected_rules(revision: &str, rules: &[SelectedTunnelRule]) -> Self {
        let mut ready = Self::empty(revision);
        for rule in rules {
            match rule.role.as_str() {
                "ingress" => ready.ingress_rule_ids.push(rule.id),
                "egress" => ready.egress_rule_ids.push(rule.id),
                "both" => {
                    ready.ingress_rule_ids.push(rule.id);
                    ready.egress_rule_ids.push(rule.id);
                }
                _ => {}
            }
        }
        ready.ingress_rule_ids.sort_unstable();
        ready.ingress_rule_ids.dedup();
        ready.egress_rule_ids.sort_unstable();
        ready.egress_rule_ids.dedup();
        ready
    }
}

#[derive(Clone, Debug)]
pub struct SharedTunnelDataReadyState {
    inner: Arc<Mutex<TunnelDataReadyState>>,
}

impl SharedTunnelDataReadyState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(TunnelDataReadyState::empty(""))),
        }
    }

    pub fn snapshot(&self) -> TunnelDataReadyState {
        self.inner
            .lock()
            .map(|ready| ready.clone())
            .unwrap_or_else(|_| TunnelDataReadyState::empty(""))
    }

    pub fn update_from_selected_rules(&self, revision: &str, rules: &[SelectedTunnelRule]) {
        if let Ok(mut ready) = self.inner.lock() {
            *ready = TunnelDataReadyState::from_selected_rules(revision, rules);
        }
    }
}

impl Default for SharedTunnelDataReadyState {
    fn default() -> Self {
        Self::new()
    }
}

pub trait TunnelDataReadySource {
    fn current_ready(&self) -> TunnelDataReadyState;
}

impl TunnelDataReadySource for TunnelDataReadyState {
    fn current_ready(&self) -> TunnelDataReadyState {
        self.clone()
    }
}

impl TunnelDataReadySource for SharedTunnelDataReadyState {
    fn current_ready(&self) -> TunnelDataReadyState {
        self.snapshot()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TunnelDataRuleFailure {
    pub rule_id: u64,
    pub status: String,
    pub error: String,
}

pub trait TunnelDataSocket {
    fn send_frame(&mut self, frame: &[u8]) -> Result<(), TransportError>;
    fn read_frame(&mut self) -> Result<Vec<u8>, TransportError>;
    fn read_optional_frame(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        self.read_frame().map(Some)
    }
    fn read_optional_frame_with_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<Vec<u8>>, TransportError> {
        let _ = timeout;
        self.read_optional_frame()
    }
}

pub trait TunnelDataTransport {
    type Socket: TunnelDataSocket;

    fn connect_tunnel_data(
        &mut self,
        url: &str,
        headers: &[HeaderPair],
    ) -> Result<Self::Socket, TransportError>;
}

#[derive(Debug, Clone)]
pub struct KtpEncryptedTcpTunnelDataTransport {
    auth: KtpEncryptedTcpTunnelDataAuth,
    read_timeout: Duration,
    max_payload_len: usize,
    max_buffer_len: usize,
}

#[derive(Debug, Clone)]
enum KtpEncryptedTcpTunnelDataAuth {
    StaticKey(KtpCryptoKey),
    Token(String),
}

impl KtpEncryptedTcpTunnelDataTransport {
    pub fn new(key: KtpCryptoKey) -> Self {
        Self {
            auth: KtpEncryptedTcpTunnelDataAuth::StaticKey(key),
            read_timeout: Duration::from_secs(2),
            max_payload_len: KTP_MAX_PAYLOAD_LEN,
            max_buffer_len: 1024 * 1024,
        }
    }

    pub fn new_with_token(token: &str) -> Self {
        Self {
            auth: KtpEncryptedTcpTunnelDataAuth::Token(token.trim().to_string()),
            read_timeout: Duration::from_secs(2),
            max_payload_len: KTP_MAX_PAYLOAD_LEN,
            max_buffer_len: 1024 * 1024,
        }
    }

    pub fn with_read_timeout(mut self, timeout: Duration) -> Self {
        self.read_timeout = timeout;
        self
    }
}

impl TunnelDataTransport for KtpEncryptedTcpTunnelDataTransport {
    type Socket = KtpEncryptedTcpTunnelDataSocket;

    fn connect_tunnel_data(
        &mut self,
        url: &str,
        _headers: &[HeaderPair],
    ) -> Result<Self::Socket, TransportError> {
        let address = parse_ktp_tcp_tunnel_data_address(url)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
        let stream = runtime
            .block_on(TokioTcpStream::connect(&address))
            .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
        let (stream, key) = match &self.auth {
            KtpEncryptedTcpTunnelDataAuth::StaticKey(key) => (stream, key.clone()),
            KtpEncryptedTcpTunnelDataAuth::Token(token) => {
                let nonce = random_ktp_tcp_auth_nonce()?;
                let preface = build_ktp_tcp_auth_preface(token, nonce)?;
                let mut stream = stream;
                runtime
                    .block_on(stream.write_all(&preface))
                    .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
                (stream, derive_ktp_tcp_crypto_key(token, nonce))
            }
        };
        let stream = KtpEncryptedTcpStream::from_stream(
            stream,
            key,
            KtpCryptoDirection::ClientToRelay,
            KtpCryptoDirection::RelayToClient,
            self.max_payload_len,
            self.max_buffer_len,
        );
        Ok(KtpEncryptedTcpTunnelDataSocket {
            runtime,
            stream,
            read_timeout: self.read_timeout,
            max_payload_len: self.max_payload_len,
        })
    }
}

pub fn build_ktp_tcp_auth_preface(token: &str, nonce: [u8; 16]) -> Result<Vec<u8>, TransportError> {
    let token = token.trim();
    if token.is_empty() {
        return Err(TransportError::EmptyToken);
    }

    let fingerprint = Sha256::digest(token.as_bytes());
    let mut preface = Vec::with_capacity(84);
    preface.extend_from_slice(b"KTA1");
    preface.extend_from_slice(&nonce);
    preface.extend_from_slice(&fingerprint);
    let mut mac = Hmac::<Sha256>::new_from_slice(token.as_bytes())
        .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
    mac.update(b"kelicloud ktp tcp auth v1");
    mac.update(&nonce);
    mac.update(&fingerprint);
    preface.extend_from_slice(&mac.finalize().into_bytes());
    Ok(preface)
}

pub fn derive_ktp_tcp_crypto_key(token: &str, nonce: [u8; 16]) -> KtpCryptoKey {
    let mut hash = Sha256::new();
    hash.update(b"kelicloud ktp tcp data v1");
    hash.update(token.trim().as_bytes());
    hash.update(nonce);
    let digest = hash.finalize();
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&digest);
    KtpCryptoKey::from_bytes(bytes)
}

fn random_ktp_tcp_auth_nonce() -> Result<[u8; 16], TransportError> {
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce)
        .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
    Ok(nonce)
}

pub struct KtpEncryptedTcpTunnelDataSocket {
    runtime: Runtime,
    stream: KtpEncryptedTcpStream,
    read_timeout: Duration,
    max_payload_len: usize,
}

impl TunnelDataSocket for KtpEncryptedTcpTunnelDataSocket {
    fn send_frame(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        let frame = decode_frame(frame, self.max_payload_len)
            .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
        self.runtime
            .block_on(self.stream.send_frame(&frame))
            .map_err(ktp_tcp_transport_error_to_transport)
    }

    fn read_frame(&mut self) -> Result<Vec<u8>, TransportError> {
        let frame = self
            .runtime
            .block_on(self.stream.next_frame())
            .map_err(ktp_tcp_transport_error_to_transport)?;
        encode_frame(&frame).map_err(|error| TransportError::RequestFailed(error.to_string()))
    }

    fn read_optional_frame(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        self.read_optional_frame_with_timeout(self.read_timeout)
    }

    fn read_optional_frame_with_timeout(
        &mut self,
        timeout_duration: Duration,
    ) -> Result<Option<Vec<u8>>, TransportError> {
        let result = self
            .runtime
            .block_on(timeout(timeout_duration, self.stream.next_frame()));
        match result {
            Ok(Ok(frame)) => encode_frame(&frame)
                .map(Some)
                .map_err(|error| TransportError::RequestFailed(error.to_string())),
            Ok(Err(error)) => Err(ktp_tcp_transport_error_to_transport(error)),
            Err(_) => Ok(None),
        }
    }
}

fn parse_ktp_tcp_tunnel_data_address(url: &str) -> Result<String, TransportError> {
    let trimmed = url.trim();
    let Some(rest) = trimmed
        .strip_prefix("ktp+tcp://")
        .or_else(|| trimmed.strip_prefix("tcp://"))
    else {
        return Err(TransportError::UnsupportedScheme(trimmed.to_string()));
    };
    let address = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if address.is_empty() {
        return Err(TransportError::EmptyEndpoint);
    }
    Ok(address)
}

fn ktp_tcp_transport_error_to_transport(error: KtpTcpTransportError) -> TransportError {
    match error {
        KtpTcpTransportError::Closed => TransportError::SocketClosed,
        other => TransportError::RequestFailed(format!("ktp tcp tunnel data error: {other}")),
    }
}

#[derive(Debug, Default, Clone)]
pub struct TungsteniteTunnelDataTransport {
    custom_dns: String,
}

impl TungsteniteTunnelDataTransport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn new_with_custom_dns(custom_dns: &str) -> Self {
        Self {
            custom_dns: custom_dns.trim().to_string(),
        }
    }
}

impl TunnelDataTransport for TungsteniteTunnelDataTransport {
    type Socket = TungsteniteTunnelDataSocket;

    fn connect_tunnel_data(
        &mut self,
        url: &str,
        headers: &[HeaderPair],
    ) -> Result<Self::Socket, TransportError> {
        let mut request = url
            .into_client_request()
            .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
        for (name, value) in headers {
            let header_name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
            let header_value = HeaderValue::from_str(value)
                .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
            request.headers_mut().insert(header_name, header_value);
        }

        let (socket, _response) = connect_websocket_request(request, &self.custom_dns)?;
        Ok(TungsteniteTunnelDataSocket {
            socket,
            read_timeout: Duration::from_secs(2),
        })
    }
}

pub struct TungsteniteTunnelDataSocket {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    read_timeout: Duration,
}

impl TunnelDataSocket for TungsteniteTunnelDataSocket {
    fn send_frame(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        self.socket
            .send(Message::Binary(frame.to_vec().into()))
            .map_err(|error| TransportError::RequestFailed(error.to_string()))
    }

    fn read_frame(&mut self) -> Result<Vec<u8>, TransportError> {
        self.read_next_frame(false, self.read_timeout)?
            .ok_or_else(|| TransportError::RequestFailed("tunnel data frame timeout".to_string()))
    }

    fn read_optional_frame(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        self.read_optional_frame_with_timeout(self.read_timeout)
    }

    fn read_optional_frame_with_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<Vec<u8>>, TransportError> {
        self.read_next_frame(true, timeout)
    }
}

impl TungsteniteTunnelDataSocket {
    fn read_next_frame(
        &mut self,
        timeout_as_idle: bool,
        read_timeout: Duration,
    ) -> Result<Option<Vec<u8>>, TransportError> {
        self.set_read_timeout(Some(read_timeout))?;
        loop {
            match self.socket.read() {
                Ok(Message::Binary(bytes)) => return Ok(Some(bytes.to_vec())),
                Ok(Message::Text(text)) => return Ok(Some(text.to_string().into_bytes())),
                Ok(Message::Close(_)) => return Err(TransportError::SocketClosed),
                Ok(_) => continue,
                Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::Interrupted => {
                    continue;
                }
                Err(tungstenite::Error::Io(error)) if is_idle_read_error(error.kind()) => {
                    if timeout_as_idle {
                        return Ok(None);
                    }
                    return Err(TransportError::RequestFailed(
                        "tunnel data frame timeout".to_string(),
                    ));
                }
                Err(error) => return Err(TransportError::RequestFailed(error.to_string())),
            }
        }
    }

    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> Result<(), TransportError> {
        match self.socket.get_mut() {
            MaybeTlsStream::Plain(stream) => stream.set_read_timeout(timeout),
            MaybeTlsStream::Rustls(stream) => stream.sock.set_read_timeout(timeout),
            _ => Ok(()),
        }
        .map_err(|error| TransportError::RequestFailed(error.to_string()))
    }
}

fn is_idle_read_error(kind: ErrorKind) -> bool {
    matches!(kind, ErrorKind::WouldBlock | ErrorKind::TimedOut)
}

#[cfg(test)]
mod tests {
    use super::is_idle_read_error;
    use std::io::ErrorKind;

    #[test]
    fn data_read_timeout_errors_are_idle_but_interrupted_retries() {
        assert!(is_idle_read_error(ErrorKind::TimedOut));
        assert!(is_idle_read_error(ErrorKind::WouldBlock));
        assert!(!is_idle_read_error(ErrorKind::Interrupted));
        assert!(!is_idle_read_error(ErrorKind::ConnectionReset));
    }
}

pub fn run_tunnel_data_once<T>(
    url: &str,
    headers: &[HeaderPair],
    agent_id_hint: &str,
    agent_version: &str,
    ready: &TunnelDataReadyState,
    transport: &mut T,
) -> Result<(), TransportError>
where
    T: TunnelDataTransport,
{
    let mut socket = match transport.connect_tunnel_data(url, headers) {
        Ok(socket) => socket,
        Err(error) if is_nonfatal_connect_error(&error) => return Ok(()),
        Err(error) => return Err(error),
    };

    let hello_payload = encode_hello_payload(agent_id_hint, agent_version, &ready.revision)?;
    let hello_frame = encode_frame(&KtpFrame::connection(FrameType::Hello, hello_payload))
        .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
    if send_tunnel_data_frame(&mut socket, &hello_frame)? == SendFrameOutcome::Closed {
        return Ok(());
    }

    if read_tunnel_data_hello_ack(&mut socket)? == ReadFrameOutcome::Closed {
        return Ok(());
    }

    let ready_payload = encode_ready_payload(ready)?;
    let ready_frame = encode_frame(&KtpFrame::connection(FrameType::Ready, ready_payload))
        .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
    if send_tunnel_data_frame(&mut socket, &ready_frame)? == SendFrameOutcome::Closed {
        return Ok(());
    }

    Ok(())
}

pub fn run_tunnel_data_session<T>(
    url: &str,
    headers: &[HeaderPair],
    agent_id_hint: &str,
    agent_version: &str,
    ready: &TunnelDataReadyState,
    transport: &mut T,
) -> Result<(), TransportError>
where
    T: TunnelDataTransport,
{
    run_tunnel_data_session_with_ready_source(
        url,
        headers,
        agent_id_hint,
        agent_version,
        ready,
        transport,
    )
}

pub fn run_tunnel_data_session_with_ready_source<T, S>(
    url: &str,
    headers: &[HeaderPair],
    agent_id_hint: &str,
    agent_version: &str,
    ready_source: &S,
    transport: &mut T,
) -> Result<(), TransportError>
where
    T: TunnelDataTransport,
    S: TunnelDataReadySource,
{
    let mut runtime = NoopTunnelSessionRuntime;
    run_tunnel_data_session_with_ready_source_and_runtime(
        url,
        headers,
        agent_id_hint,
        agent_version,
        ready_source,
        transport,
        &mut runtime,
    )
}

pub fn run_tunnel_data_session_with_ready_source_and_runtime<T, S, R>(
    url: &str,
    headers: &[HeaderPair],
    agent_id_hint: &str,
    agent_version: &str,
    ready_source: &S,
    transport: &mut T,
    runtime: &mut R,
) -> Result<(), TransportError>
where
    T: TunnelDataTransport,
    S: TunnelDataReadySource,
    R: TunnelSessionRuntime,
{
    let mut socket = match transport.connect_tunnel_data(url, headers) {
        Ok(socket) => socket,
        Err(error) if is_nonfatal_connect_error(&error) => return Ok(()),
        Err(error) => return Err(error),
    };

    let hello_ready = ready_source.current_ready();
    let hello_payload = encode_hello_payload(agent_id_hint, agent_version, &hello_ready.revision)?;
    let hello_frame = encode_frame(&KtpFrame::connection(FrameType::Hello, hello_payload))
        .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
    if send_tunnel_data_frame(&mut socket, &hello_frame)? == SendFrameOutcome::Closed {
        return Ok(());
    }

    if read_tunnel_data_hello_ack(&mut socket)? == ReadFrameOutcome::Closed {
        return Ok(());
    }

    runtime.tick()?;
    let mut last_ready = ready_source.current_ready();
    if send_ready_frame(&mut socket, &last_ready)? == SendFrameOutcome::Closed {
        return Ok(());
    }

    loop {
        runtime.tick()?;
        let current_ready = ready_source.current_ready();
        if current_ready != last_ready {
            if send_ready_frame(&mut socket, &current_ready)? == SendFrameOutcome::Closed {
                return Ok(());
            }
            last_ready = current_ready;
        }
        let sent_runtime_frames = drain_tunnel_session_runtime_frames(&mut socket, runtime)?;
        if !sent_runtime_frames {
            if let Some(timeout) = runtime.tunnel_data_client_frame_wait_timeout() {
                if drain_tunnel_session_runtime_frames_after_wait(&mut socket, runtime, timeout)? {
                    continue;
                }
            }
        }
        let read_result = match runtime.tunnel_data_socket_idle_timeout() {
            Some(timeout) => socket.read_optional_frame_with_timeout(timeout),
            None => socket.read_optional_frame(),
        };
        match read_result {
            Ok(Some(bytes)) => handle_tunnel_data_session_frame(&mut socket, &bytes, runtime)?,
            Ok(None) => continue,
            Err(TransportError::SocketClosed) => return Ok(()),
            Err(error) => return Err(error),
        }
    }
}

fn send_ready_frame<S>(
    socket: &mut S,
    ready: &TunnelDataReadyState,
) -> Result<SendFrameOutcome, TransportError>
where
    S: TunnelDataSocket,
{
    let ready_payload = encode_ready_payload(ready)?;
    let ready_frame = encode_frame(&KtpFrame::connection(FrameType::Ready, ready_payload))
        .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
    send_tunnel_data_frame(socket, &ready_frame)
}

pub fn tunnel_data_startup_line(url: &str, enabled: bool) -> String {
    if !enabled {
        return "tunnel data: disabled".to_string();
    }

    format!("tunnel data: enabled url={}", redact_token_in_url(url))
}

fn is_nonfatal_connect_error(error: &TransportError) -> bool {
    match error {
        TransportError::RequestFailed(message) => {
            let message = message.to_ascii_lowercase();
            message.contains("404")
                || message.contains("403")
                || message.contains("feature_disabled")
        }
        TransportError::SocketClosed => true,
        TransportError::InvalidClientToken { .. }
        | TransportError::EmptyEndpoint
        | TransportError::EmptyToken
        | TransportError::UnsupportedScheme(_) => false,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SendFrameOutcome {
    Sent,
    Closed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReadFrameOutcome {
    Frame,
    Closed,
}

fn send_tunnel_data_frame<S>(
    socket: &mut S,
    frame: &[u8],
) -> Result<SendFrameOutcome, TransportError>
where
    S: TunnelDataSocket,
{
    match socket.send_frame(frame) {
        Ok(()) => Ok(SendFrameOutcome::Sent),
        Err(TransportError::SocketClosed) => Ok(SendFrameOutcome::Closed),
        Err(error) => Err(error),
    }
}

fn read_tunnel_data_hello_ack<S>(socket: &mut S) -> Result<ReadFrameOutcome, TransportError>
where
    S: TunnelDataSocket,
{
    let bytes = match socket.read_frame() {
        Ok(bytes) => bytes,
        Err(TransportError::SocketClosed) => return Ok(ReadFrameOutcome::Closed),
        Err(error) => return Err(error),
    };
    let frame = decode_frame(&bytes, KTP_MAX_PAYLOAD_LEN)
        .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
    if frame.frame_type != FrameType::HelloAck {
        return Err(TransportError::RequestFailed(format!(
            "expected tunnel data hello_ack, got {:?}",
            frame.frame_type
        )));
    }
    Ok(ReadFrameOutcome::Frame)
}

fn drain_tunnel_session_runtime_frames<S, R>(
    socket: &mut S,
    runtime: &mut R,
) -> Result<bool, TransportError>
where
    S: TunnelDataSocket,
    R: TunnelSessionRuntime,
{
    let mut sent_any = false;
    loop {
        let frames = runtime.next_client_frames(64)?;
        if frames.is_empty() {
            return Ok(sent_any);
        }
        send_tunnel_session_runtime_frame_batch(socket, frames)?;
        sent_any = true;
    }
}

fn drain_tunnel_session_runtime_frames_after_wait<S, R>(
    socket: &mut S,
    runtime: &mut R,
    timeout: Duration,
) -> Result<bool, TransportError>
where
    S: TunnelDataSocket,
    R: TunnelSessionRuntime,
{
    let frames = runtime.next_client_frames_after_wait(64, timeout)?;
    if frames.is_empty() {
        return Ok(false);
    }
    send_tunnel_session_runtime_frame_batch(socket, frames)?;
    Ok(true)
}

fn send_tunnel_session_runtime_frame_batch<S>(
    socket: &mut S,
    frames: Vec<KtpFrame>,
) -> Result<(), TransportError>
where
    S: TunnelDataSocket,
{
    for frame in frames {
        let bytes = encode_frame(&frame)
            .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
        let _ = send_tunnel_data_frame(socket, &bytes)?;
    }
    Ok(())
}

fn handle_tunnel_data_session_frame<S, R>(
    socket: &mut S,
    bytes: &[u8],
    runtime: &mut R,
) -> Result<(), TransportError>
where
    S: TunnelDataSocket,
    R: TunnelSessionRuntime,
{
    let frame = decode_frame(bytes, KTP_MAX_PAYLOAD_LEN)
        .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
    match frame.frame_type {
        FrameType::Ping => {
            let pong = encode_frame(&KtpFrame::connection(FrameType::Pong, frame.payload))
                .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
            let _ = send_tunnel_data_frame(socket, &pong)?;
        }
        FrameType::SessionOpen
        | FrameType::SessionAccept
        | FrameType::SessionData
        | FrameType::SessionWindow
        | FrameType::SessionClose
        | FrameType::SessionError => {
            for response in runtime.handle_server_frame(frame)? {
                let bytes = encode_frame(&response)
                    .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
                let _ = send_tunnel_data_frame(socket, &bytes)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn encode_hello_payload(
    agent_id_hint: &str,
    agent_version: &str,
    revision: &str,
) -> Result<Vec<u8>, TransportError> {
    let mut payload = Vec::new();
    write_string(&mut payload, agent_id_hint)?;
    write_string(&mut payload, agent_version)?;
    write_string(&mut payload, revision)?;
    write_string_list(&mut payload, &["tcp", "multiplex", "flow_control", "stats"])?;
    Ok(payload)
}

fn encode_ready_payload(ready: &TunnelDataReadyState) -> Result<Vec<u8>, TransportError> {
    let mut payload = Vec::new();
    write_string(&mut payload, &ready.revision)?;
    write_u64_list(&mut payload, &ready.ingress_rule_ids)?;
    write_u64_list(&mut payload, &ready.egress_rule_ids)?;
    write_count(&mut payload, ready.failed_rules.len(), "failed rule count")?;
    for failure in &ready.failed_rules {
        payload.extend_from_slice(&failure.rule_id.to_be_bytes());
        write_string(&mut payload, &failure.status)?;
        write_string(&mut payload, &failure.error)?;
    }
    Ok(payload)
}

fn write_string(output: &mut Vec<u8>, value: &str) -> Result<(), TransportError> {
    let bytes = value.as_bytes();
    write_count(output, bytes.len(), "string length")?;
    output.extend_from_slice(bytes);
    Ok(())
}

fn write_string_list(output: &mut Vec<u8>, values: &[&str]) -> Result<(), TransportError> {
    write_count(output, values.len(), "string list count")?;
    for value in values {
        write_string(output, value)?;
    }
    Ok(())
}

fn write_u64_list(output: &mut Vec<u8>, values: &[u64]) -> Result<(), TransportError> {
    write_count(output, values.len(), "u64 list count")?;
    for value in values {
        output.extend_from_slice(&value.to_be_bytes());
    }
    Ok(())
}

fn write_count(output: &mut Vec<u8>, len: usize, field: &str) -> Result<(), TransportError> {
    let len = u16::try_from(len)
        .map_err(|_| TransportError::RequestFailed(format!("{field} too long: exceeds u16")))?;
    output.extend_from_slice(&len.to_be_bytes());
    Ok(())
}

fn redact_token_in_url(url: &str) -> String {
    let Some((base, query)) = url.split_once('?') else {
        return url.to_string();
    };

    let redacted_query = query
        .split('&')
        .map(|part| {
            part.split_once('=')
                .filter(|(key, _)| *key == "token")
                .map(|(key, _)| format!("{key}=redacted"))
                .unwrap_or_else(|| part.to_string())
        })
        .collect::<Vec<_>>()
        .join("&");

    format!("{base}?{redacted_query}")
}
