use std::error::Error;
use std::fmt;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::time::Duration;

use crate::config::AgentConfig;
use crate::ping::PingResult;
use crate::protocol::BackendMessage;
use crate::report::{BasicInfo, Report};
use tungstenite::client::IntoClientRequest;
use tungstenite::http::{HeaderName, HeaderValue};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

pub type HeaderPair = (String, String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    EmptyEndpoint,
    EmptyToken,
    UnsupportedScheme(String),
    RequestFailed(String),
    SocketClosed,
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyEndpoint => write!(f, "endpoint is required"),
            Self::EmptyToken => write!(f, "token is required"),
            Self::UnsupportedScheme(scheme) => write!(f, "unsupported endpoint scheme: {scheme}"),
            Self::RequestFailed(message) => write!(f, "request failed: {message}"),
            Self::SocketClosed => write!(f, "websocket closed"),
        }
    }
}

impl Error for TransportError {}

pub trait HttpTransport {
    fn upload_basic_info(
        &mut self,
        url: &str,
        headers: &[HeaderPair],
        basic_info: &BasicInfo,
    ) -> Result<(), TransportError>;
}

pub trait ReportSocket {
    fn send_report(&mut self, report: &Report) -> Result<(), TransportError>;
    fn read_message(&mut self) -> Result<Option<Vec<u8>>, TransportError>;
    fn send_ping(&mut self) -> Result<(), TransportError>;
    fn send_ping_result(&mut self, result: &PingResult) -> Result<(), TransportError>;
}

pub trait WebSocketTransport {
    type Socket: ReportSocket;

    fn connect_report(
        &mut self,
        url: &str,
        headers: &[HeaderPair],
    ) -> Result<Self::Socket, TransportError>;
}

pub fn build_basic_info_url(endpoint: &str, token: &str) -> Result<String, TransportError> {
    let endpoint = normalize_http_base(endpoint)?;
    let token = require_non_empty(token, TransportError::EmptyToken)?;
    Ok(format!(
        "{endpoint}/api/clients/uploadBasicInfo?token={}",
        percent_encode(token)
    ))
}

pub fn access_headers(config: &AgentConfig) -> Vec<HeaderPair> {
    let id = config.cf_access_client_id.trim();
    let secret = config.cf_access_client_secret.trim();
    if id.is_empty() || secret.is_empty() {
        return Vec::new();
    }

    vec![
        ("CF-Access-Client-Id".to_string(), id.to_string()),
        ("CF-Access-Client-Secret".to_string(), secret.to_string()),
    ]
}

pub fn parse_socket_message(bytes: &[u8]) -> BackendMessage {
    crate::protocol::parse_backend_message(bytes)
        .unwrap_or(BackendMessage::Unknown { message: None })
}

pub struct ReqwestHttpTransport {
    client: reqwest::blocking::Client,
}

impl ReqwestHttpTransport {
    pub fn new(insecure: bool) -> Result<Self, TransportError> {
        let client = reqwest::blocking::Client::builder()
            .danger_accept_invalid_certs(insecure)
            .build()
            .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
        Ok(Self { client })
    }
}

impl HttpTransport for ReqwestHttpTransport {
    fn upload_basic_info(
        &mut self,
        url: &str,
        headers: &[HeaderPair],
        basic_info: &BasicInfo,
    ) -> Result<(), TransportError> {
        let mut request = self.client.post(url).json(basic_info);
        for (name, value) in headers {
            request = request.header(name, value);
        }

        let response = request
            .send()
            .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
        if response.status().is_success() {
            return Ok(());
        }

        let status = response.status();
        let body = response.text().unwrap_or_default();
        Err(TransportError::RequestFailed(format!(
            "status={status} {body}"
        )))
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct TungsteniteWebSocketTransport;

impl WebSocketTransport for TungsteniteWebSocketTransport {
    type Socket = TungsteniteReportSocket;

    fn connect_report(
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

        let (socket, _response) = tungstenite::connect(request)
            .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
        Ok(TungsteniteReportSocket {
            socket,
            read_timeout: Duration::from_millis(100),
        })
    }
}

pub struct TungsteniteReportSocket {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    read_timeout: Duration,
}

impl ReportSocket for TungsteniteReportSocket {
    fn send_report(&mut self, report: &Report) -> Result<(), TransportError> {
        let payload = serde_json::to_string(report)
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
            Err(tungstenite::Error::Io(error))
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                Ok(None)
            }
            Err(error) => Err(TransportError::RequestFailed(error.to_string())),
        }
    }

    fn send_ping(&mut self) -> Result<(), TransportError> {
        self.socket
            .send(Message::Ping(Vec::new().into()))
            .map_err(|error| TransportError::RequestFailed(error.to_string()))
    }

    fn send_ping_result(&mut self, result: &PingResult) -> Result<(), TransportError> {
        let payload = serde_json::to_string(result)
            .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
        self.socket
            .send(Message::Text(payload.into()))
            .map_err(|error| TransportError::RequestFailed(error.to_string()))
    }
}

impl TungsteniteReportSocket {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> Result<(), TransportError> {
        match self.socket.get_mut() {
            MaybeTlsStream::Plain(stream) => stream.set_read_timeout(timeout),
            MaybeTlsStream::Rustls(stream) => stream.sock.set_read_timeout(timeout),
            _ => Ok(()),
        }
        .map_err(|error| TransportError::RequestFailed(error.to_string()))
    }
}

fn normalize_http_base(endpoint: &str) -> Result<String, TransportError> {
    let endpoint = endpoint.trim().trim_end_matches('/');
    if endpoint.is_empty() {
        return Err(TransportError::EmptyEndpoint);
    }

    if endpoint.starts_with("https://") || endpoint.starts_with("http://") {
        return Ok(endpoint.to_string());
    }

    let scheme = endpoint
        .split_once("://")
        .map(|(scheme, _)| scheme.to_string())
        .unwrap_or_else(|| "missing".to_string());
    Err(TransportError::UnsupportedScheme(scheme))
}

fn require_non_empty(value: &str, error: TransportError) -> Result<&str, TransportError> {
    let value = value.trim();
    if value.is_empty() {
        Err(error)
    } else {
        Ok(value)
    }
}

fn percent_encode(value: &str) -> String {
    let mut output = String::new();
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                output.push(*byte as char)
            }
            _ => output.push_str(&format!("%{byte:02X}")),
        }
    }
    output
}
