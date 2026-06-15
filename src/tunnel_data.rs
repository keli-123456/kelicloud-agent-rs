use crate::ktp::{decode_frame, encode_frame, FrameType, KtpFrame, KTP_MAX_PAYLOAD_LEN};
use crate::transport::{connect_websocket_request, HeaderPair, TransportError};
use std::io::ErrorKind;
use std::net::TcpStream;
use std::time::Duration;
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
}

pub trait TunnelDataTransport {
    type Socket: TunnelDataSocket;

    fn connect_tunnel_data(
        &mut self,
        url: &str,
        headers: &[HeaderPair],
    ) -> Result<Self::Socket, TransportError>;
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
        self.set_read_timeout(Some(self.read_timeout))?;
        loop {
            match self.socket.read() {
                Ok(Message::Binary(bytes)) => return Ok(bytes.to_vec()),
                Ok(Message::Text(text)) => return Ok(text.to_string().into_bytes()),
                Ok(Message::Close(_)) => return Err(TransportError::SocketClosed),
                Ok(_) => continue,
                Err(tungstenite::Error::Io(error))
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    return Err(TransportError::RequestFailed(
                        "tunnel data hello_ack timeout".to_string(),
                    ));
                }
                Err(error) => return Err(TransportError::RequestFailed(error.to_string())),
            }
        }
    }
}

impl TungsteniteTunnelDataSocket {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> Result<(), TransportError> {
        match self.socket.get_mut() {
            MaybeTlsStream::Plain(stream) => stream.set_read_timeout(timeout),
            MaybeTlsStream::Rustls(stream) => stream.sock.set_read_timeout(timeout),
            _ => Ok(()),
        }
        .map_err(|error| TransportError::RequestFailed(error.to_string()))
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
