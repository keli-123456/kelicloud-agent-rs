use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::time::{Duration, Instant};

use crate::transport::{connect_websocket_request, HeaderPair, TransportError};
use tungstenite::client::IntoClientRequest;
use tungstenite::http::{HeaderName, HeaderValue};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

pub const TUNNEL_CONTROL_PROTOCOL_V1: &str = "keli-tunnel-control.v1";
pub const TUNNEL_DATA_TRANSPORT_WEBSOCKET: &str = "websocket";
pub const TUNNEL_DATA_TRANSPORT_KTP_TCP: &str = "ktp_tcp";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedTunnelRule {
    pub id: u64,
    pub name: String,
    pub enabled: bool,
    pub protocol: String,
    pub role: String,
    pub ingress_group: String,
    pub listen_address: String,
    pub listen_port: u16,
    pub egress_group: String,
    pub target_host: String,
    pub target_port: u16,
    pub source_allowlist: String,
    pub max_concurrent_sessions: u32,
    pub last_revision: i64,
    #[serde(default = "default_tunnel_data_transport")]
    pub data_transport: String,
}

impl SelectedTunnelRule {
    pub fn data_transport(&self) -> &str {
        let value = self.data_transport.trim();
        if value.is_empty() {
            TUNNEL_DATA_TRANSPORT_WEBSOCKET
        } else {
            value
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectedTunnelRule {
    pub id: u64,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TunnelRuleStatus {
    pub id: u64,
    pub status: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TunnelControlClientMessage {
    Hello {
        control_protocol: String,
        agent_version: String,
        capabilities: Vec<String>,
        data_transports: Vec<String>,
        data_plane: bool,
    },
    Heartbeat {
        last_rule_revision: String,
        active_rules: Vec<u64>,
    },
    RuleAck {
        revision: String,
        accepted_rule_ids: Vec<u64>,
        rejected_rules: Vec<RejectedTunnelRule>,
    },
    RuleStatus {
        revision: String,
        rules: Vec<TunnelRuleStatus>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TunnelControlServerMessage {
    HelloAck {
        server_protocol: String,
        heartbeat_interval_seconds: u64,
    },
    RuleSync {
        revision: String,
        rules: Vec<SelectedTunnelRule>,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelControlError {
    InvalidMessage(String),
}

impl fmt::Display for TunnelControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMessage(message) => write!(f, "invalid tunnel control message: {message}"),
        }
    }
}

impl Error for TunnelControlError {}

pub fn build_hello(agent_version: &str) -> TunnelControlClientMessage {
    TunnelControlClientMessage::Hello {
        control_protocol: TUNNEL_CONTROL_PROTOCOL_V1.to_string(),
        agent_version: agent_version.trim().to_string(),
        capabilities: vec![
            "tunnel_control".to_string(),
            "rule_sync".to_string(),
            "status_report".to_string(),
        ],
        data_transports: supported_tunnel_data_transports(),
        data_plane: false,
    }
}

pub fn supported_tunnel_data_transports() -> Vec<String> {
    vec![
        TUNNEL_DATA_TRANSPORT_WEBSOCKET.to_string(),
        TUNNEL_DATA_TRANSPORT_KTP_TCP.to_string(),
    ]
}

fn default_tunnel_data_transport() -> String {
    TUNNEL_DATA_TRANSPORT_WEBSOCKET.to_string()
}

pub fn build_heartbeat(revision: &str, active_rules: &[u64]) -> TunnelControlClientMessage {
    TunnelControlClientMessage::Heartbeat {
        last_rule_revision: revision.trim().to_string(),
        active_rules: active_rules.to_vec(),
    }
}

pub fn build_rule_ack(
    revision: &str,
    accepted_rules: &[SelectedTunnelRule],
    rejected_rules: &[RejectedTunnelRule],
) -> TunnelControlClientMessage {
    TunnelControlClientMessage::RuleAck {
        revision: revision.trim().to_string(),
        accepted_rule_ids: accepted_rules.iter().map(|rule| rule.id).collect(),
        rejected_rules: rejected_rules.to_vec(),
    }
}

pub fn build_rule_status(revision: &str, rules: &[TunnelRuleStatus]) -> TunnelControlClientMessage {
    TunnelControlClientMessage::RuleStatus {
        revision: revision.trim().to_string(),
        rules: rules.to_vec(),
    }
}

pub fn parse_server_message(
    bytes: &[u8],
) -> Result<TunnelControlServerMessage, TunnelControlError> {
    serde_json::from_slice(bytes)
        .map_err(|error| TunnelControlError::InvalidMessage(error.to_string()))
}

pub trait TunnelControlSocket {
    fn send_message(&mut self, message: &TunnelControlClientMessage) -> Result<(), TransportError>;
    fn read_message(&mut self) -> Result<Option<Vec<u8>>, TransportError>;
}

pub trait TunnelControlTransport {
    type Socket: TunnelControlSocket;

    fn connect_tunnel_control(
        &mut self,
        url: &str,
        headers: &[HeaderPair],
    ) -> Result<Self::Socket, TransportError>;
}

pub trait TunnelRuleStateSink {
    fn update_rules(&self, revision: &str, rules: &[SelectedTunnelRule]);
}

impl TunnelRuleStateSink for crate::tunnel_data::SharedTunnelDataReadyState {
    fn update_rules(&self, revision: &str, rules: &[SelectedTunnelRule]) {
        self.update_from_selected_rules(revision, rules);
    }
}

struct NoopTunnelRuleStateSink;

impl TunnelRuleStateSink for NoopTunnelRuleStateSink {
    fn update_rules(&self, _revision: &str, _rules: &[SelectedTunnelRule]) {}
}

pub fn is_non_fatal_tunnel_control_error(error: &TransportError) -> bool {
    match error {
        TransportError::InvalidClientToken { .. } => false,
        TransportError::EmptyEndpoint
        | TransportError::EmptyToken
        | TransportError::UnsupportedScheme(_) => false,
        TransportError::RequestFailed(message) => {
            let lower = message.to_ascii_lowercase();
            lower.contains("404") || lower.contains("403") || lower.contains("feature_disabled")
        }
        TransportError::SocketClosed => true,
    }
}

pub fn run_tunnel_control_once<T>(
    url: &str,
    headers: &[HeaderPair],
    agent_version: &str,
    transport: &mut T,
) -> Result<(), TransportError>
where
    T: TunnelControlTransport,
{
    run_tunnel_control_once_with_rule_sink(
        url,
        headers,
        agent_version,
        transport,
        &NoopTunnelRuleStateSink,
    )
}

pub fn run_tunnel_control_once_with_rule_sink<T, S>(
    url: &str,
    headers: &[HeaderPair],
    agent_version: &str,
    transport: &mut T,
    rule_sink: &S,
) -> Result<(), TransportError>
where
    T: TunnelControlTransport,
    S: TunnelRuleStateSink,
{
    let mut socket = match transport.connect_tunnel_control(url, headers) {
        Ok(socket) => socket,
        Err(error) if is_non_fatal_tunnel_control_error(&error) => return Ok(()),
        Err(error) => return Err(error),
    };

    socket.send_message(&build_hello(agent_version))?;
    let mut latest_revision = String::new();
    let mut accepted_rules = Vec::new();
    let mut heartbeat_interval = Duration::from_secs(15);

    while let Some(bytes) = socket.read_message()? {
        if handle_tunnel_control_message(
            &mut socket,
            &bytes,
            &mut latest_revision,
            &mut accepted_rules,
            &mut heartbeat_interval,
            rule_sink,
        )? == TunnelControlLoopAction::Stop
        {
            return Ok(());
        }
    }

    send_tunnel_control_heartbeat(&mut socket, &latest_revision, &accepted_rules)?;
    Ok(())
}

pub fn run_tunnel_control_session<T>(
    url: &str,
    headers: &[HeaderPair],
    agent_version: &str,
    transport: &mut T,
) -> Result<(), TransportError>
where
    T: TunnelControlTransport,
{
    run_tunnel_control_session_with_rule_sink(
        url,
        headers,
        agent_version,
        transport,
        &NoopTunnelRuleStateSink,
    )
}

pub fn run_tunnel_control_session_with_rule_sink<T, S>(
    url: &str,
    headers: &[HeaderPair],
    agent_version: &str,
    transport: &mut T,
    rule_sink: &S,
) -> Result<(), TransportError>
where
    T: TunnelControlTransport,
    S: TunnelRuleStateSink,
{
    let mut socket = match transport.connect_tunnel_control(url, headers) {
        Ok(socket) => socket,
        Err(error) if is_non_fatal_tunnel_control_error(&error) => return Ok(()),
        Err(error) => return Err(error),
    };

    socket.send_message(&build_hello(agent_version))?;
    let mut latest_revision = String::new();
    let mut accepted_rules = Vec::new();
    let mut heartbeat_interval = Duration::from_secs(15);
    let mut last_heartbeat = Instant::now();

    loop {
        match socket.read_message() {
            Ok(Some(bytes)) => {
                if handle_tunnel_control_message(
                    &mut socket,
                    &bytes,
                    &mut latest_revision,
                    &mut accepted_rules,
                    &mut heartbeat_interval,
                    rule_sink,
                )? == TunnelControlLoopAction::Stop
                {
                    return Ok(());
                }
            }
            Ok(None) => {
                if last_heartbeat.elapsed() >= heartbeat_interval {
                    send_tunnel_control_heartbeat(&mut socket, &latest_revision, &accepted_rules)?;
                    last_heartbeat = Instant::now();
                }
            }
            Err(TransportError::SocketClosed) => return Ok(()),
            Err(error) => return Err(error),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TunnelControlLoopAction {
    Continue,
    Stop,
}

fn handle_tunnel_control_message<S>(
    socket: &mut S,
    bytes: &[u8],
    latest_revision: &mut String,
    accepted_rules: &mut Vec<SelectedTunnelRule>,
    heartbeat_interval: &mut Duration,
    rule_sink: &impl TunnelRuleStateSink,
) -> Result<TunnelControlLoopAction, TransportError>
where
    S: TunnelControlSocket,
{
    match parse_server_message(bytes) {
        Ok(TunnelControlServerMessage::HelloAck {
            server_protocol,
            heartbeat_interval_seconds,
        }) => {
            if server_protocol.trim() != TUNNEL_CONTROL_PROTOCOL_V1 {
                return Err(TransportError::RequestFailed(format!(
                    "unsupported tunnel control protocol: {server_protocol}"
                )));
            }
            if heartbeat_interval_seconds > 0 {
                *heartbeat_interval = Duration::from_secs(heartbeat_interval_seconds);
            }
            Ok(TunnelControlLoopAction::Continue)
        }
        Ok(TunnelControlServerMessage::RuleSync { revision, rules }) => {
            *latest_revision = revision;
            *accepted_rules = rules;
            rule_sink.update_rules(latest_revision, accepted_rules);
            socket.send_message(&build_rule_ack(latest_revision, accepted_rules, &[]))?;
            let statuses = accepted_rules
                .iter()
                .map(|rule| TunnelRuleStatus {
                    id: rule.id,
                    status: "ok".to_string(),
                    error: String::new(),
                })
                .collect::<Vec<_>>();
            socket.send_message(&build_rule_status(latest_revision, &statuses))?;
            Ok(TunnelControlLoopAction::Continue)
        }
        Ok(TunnelControlServerMessage::Error { code, message }) => {
            if code == "feature_disabled" {
                return Ok(TunnelControlLoopAction::Stop);
            }
            Err(TransportError::RequestFailed(message))
        }
        Err(error) => Err(TransportError::RequestFailed(error.to_string())),
    }
}

fn send_tunnel_control_heartbeat<S>(
    socket: &mut S,
    latest_revision: &str,
    accepted_rules: &[SelectedTunnelRule],
) -> Result<(), TransportError>
where
    S: TunnelControlSocket,
{
    let active_rules = accepted_rules
        .iter()
        .map(|rule| rule.id)
        .collect::<Vec<_>>();
    socket.send_message(&build_heartbeat(latest_revision, &active_rules))
}

#[derive(Debug, Default, Clone)]
pub struct TungsteniteTunnelControlTransport {
    custom_dns: String,
}

impl TungsteniteTunnelControlTransport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn new_with_custom_dns(custom_dns: &str) -> Self {
        Self {
            custom_dns: custom_dns.trim().to_string(),
        }
    }
}

impl TunnelControlTransport for TungsteniteTunnelControlTransport {
    type Socket = TungsteniteTunnelControlSocket;

    fn connect_tunnel_control(
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
        Ok(TungsteniteTunnelControlSocket {
            socket,
            read_timeout: Duration::from_millis(500),
        })
    }
}

pub struct TungsteniteTunnelControlSocket {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    read_timeout: Duration,
}

impl TunnelControlSocket for TungsteniteTunnelControlSocket {
    fn send_message(&mut self, message: &TunnelControlClientMessage) -> Result<(), TransportError> {
        let payload = serde_json::to_string(message)
            .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
        self.socket
            .send(Message::Text(payload.into()))
            .map_err(|error| TransportError::RequestFailed(error.to_string()))
    }

    fn read_message(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        self.set_read_timeout(Some(self.read_timeout))?;
        match self.socket.read() {
            Ok(Message::Text(text)) => Ok(Some(text.to_string().into_bytes())),
            Ok(Message::Binary(bytes)) => Ok(Some(bytes.to_vec())),
            Ok(Message::Close(_)) => Err(TransportError::SocketClosed),
            Ok(_) => Ok(None),
            Err(tungstenite::Error::Io(error)) if is_idle_read_error(error.kind()) => Ok(None),
            Err(error) => Err(TransportError::RequestFailed(error.to_string())),
        }
    }
}

fn is_idle_read_error(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
    )
}

#[cfg(test)]
mod tests {
    use super::is_idle_read_error;
    use std::io::ErrorKind;

    #[test]
    fn interrupted_control_read_is_retryable_idle() {
        assert!(is_idle_read_error(ErrorKind::Interrupted));
        assert!(is_idle_read_error(ErrorKind::TimedOut));
        assert!(is_idle_read_error(ErrorKind::WouldBlock));
        assert!(!is_idle_read_error(ErrorKind::ConnectionReset));
    }
}

impl TungsteniteTunnelControlSocket {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> Result<(), TransportError> {
        match self.socket.get_mut() {
            MaybeTlsStream::Plain(stream) => stream.set_read_timeout(timeout),
            MaybeTlsStream::Rustls(stream) => stream.sock.set_read_timeout(timeout),
            _ => Ok(()),
        }
        .map_err(|error| TransportError::RequestFailed(error.to_string()))
    }
}

pub fn tunnel_control_startup_line(url: &str, enabled: bool) -> String {
    if !enabled {
        return "tunnel control: disabled".to_string();
    }
    format!("tunnel control: enabled url={}", redact_token_in_url(url))
}

fn redact_token_in_url(url: &str) -> String {
    let Some((base, query)) = url.split_once('?') else {
        return url.to_string();
    };
    let query = query
        .split('&')
        .map(|part| {
            if part.split_once('=').is_some_and(|(key, _)| key == "token") {
                "token=redacted".to_string()
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("&");
    format!("{base}?{query}")
}
