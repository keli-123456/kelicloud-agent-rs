use std::error::Error;
use std::fmt;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use crate::config::AgentConfig;
use crate::ping::PingResult;
use crate::protocol::BackendMessage;
use crate::report::{BasicInfo, Report};
use crate::smoke_summary::smoke_event_line;
use tungstenite::client::IntoClientRequest;
use tungstenite::http::{HeaderName, HeaderValue};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{client_tls_with_config, Message, WebSocket};

pub type HeaderPair = (String, String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    EmptyEndpoint,
    EmptyToken,
    UnsupportedScheme(String),
    InvalidClientToken {
        operation: String,
        token: String,
        status_code: u16,
        detail: String,
    },
    RequestFailed(String),
    SocketClosed,
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyEndpoint => write!(f, "endpoint is required"),
            Self::EmptyToken => write!(f, "token is required"),
            Self::UnsupportedScheme(scheme) => write!(f, "unsupported endpoint scheme: {scheme}"),
            Self::InvalidClientToken {
                operation,
                status_code,
                detail,
                ..
            } => {
                if detail.is_empty() {
                    write!(
                        f,
                        "invalid client token during {operation}: status={status_code}"
                    )
                } else {
                    write!(f, "invalid client token during {operation}: {detail}")
                }
            }
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
        Self::new_with_custom_dns(insecure, "")
    }

    pub fn from_config(config: &AgentConfig) -> Result<Self, TransportError> {
        Self::new_with_custom_dns(config.insecure, &config.custom_dns)
    }

    pub fn new_with_custom_dns(insecure: bool, custom_dns: &str) -> Result<Self, TransportError> {
        let mut builder =
            reqwest::blocking::Client::builder().danger_accept_invalid_certs(insecure);
        let custom_dns = custom_dns.trim();
        if !custom_dns.is_empty() {
            builder = builder.dns_resolver(Arc::new(crate::linux_proc::CustomDnsResolver::new(
                custom_dns,
            )));
        }
        let client = builder
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
            println!("{}", smoke_event_line("basic_info_uploaded", &[]));
            return Ok(());
        }

        let status = response.status();
        let body = response.text().unwrap_or_default();
        Err(classify_client_token_response(
            "upload basic info",
            &token_from_url(url),
            status.as_u16(),
            &body,
        ))
    }
}

#[derive(Debug, Default, Clone)]
pub struct TungsteniteWebSocketTransport {
    custom_dns: String,
}

impl TungsteniteWebSocketTransport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_config(config: &AgentConfig) -> Self {
        Self::new_with_custom_dns(&config.custom_dns)
    }

    pub fn new_with_custom_dns(custom_dns: &str) -> Self {
        Self {
            custom_dns: custom_dns.trim().to_string(),
        }
    }
}

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

        let (socket, _response) = connect_websocket_request(request, &self.custom_dns)?;
        println!("{}", smoke_event_line("report_websocket_connected", &[]));
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
            .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
        println!("{}", smoke_event_line("report_sent", &[]));
        Ok(())
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
        let task_id = result.task_id.to_string();
        let value = result.value.to_string();
        self.socket
            .send(Message::Text(payload.into()))
            .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
        println!(
            "{}",
            smoke_event_line(
                "ping_result_uploaded",
                &[("task_id", &task_id), ("value", &value)]
            )
        );
        Ok(())
    }
}

pub(crate) fn connect_websocket_request(
    request: tungstenite::handshake::client::Request,
    custom_dns: &str,
) -> Result<
    (
        WebSocket<MaybeTlsStream<TcpStream>>,
        tungstenite::handshake::client::Response,
    ),
    TransportError,
> {
    if custom_dns.trim().is_empty() {
        let token = token_from_request(&request);
        return tungstenite::connect(request)
            .map_err(|error| classify_websocket_error("connect websocket", &token, error));
    }

    connect_websocket_with_custom_dns(request, custom_dns)
}

fn connect_websocket_with_custom_dns(
    request: tungstenite::handshake::client::Request,
    custom_dns: &str,
) -> Result<
    (
        WebSocket<MaybeTlsStream<TcpStream>>,
        tungstenite::handshake::client::Response,
    ),
    TransportError,
> {
    let uri = request.uri();
    let host = uri
        .host()
        .ok_or_else(|| TransportError::RequestFailed("websocket URL missing host".to_string()))?;
    let port = uri.port_u16().unwrap_or_else(|| match uri.scheme_str() {
        Some("wss") => 443,
        _ => 80,
    });
    let mut addrs =
        crate::linux_proc::resolve_host_with_dns_server(custom_dns, host, Duration::from_secs(10))
            .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
    for addr in &mut addrs {
        addr.set_port(port);
    }

    let mut last_error = None;
    for addr in addrs {
        match TcpStream::connect(addr) {
            Ok(stream) => {
                stream
                    .set_nodelay(true)
                    .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
                let token = token_from_request(&request);
                return client_tls_with_config(request, stream, None, None).map_err(|error| {
                    match error {
                        tungstenite::handshake::HandshakeError::Failure(error) => {
                            classify_websocket_error("connect websocket", &token, error)
                        }
                        tungstenite::handshake::HandshakeError::Interrupted(_) => {
                            TransportError::RequestFailed(
                                "websocket handshake interrupted".to_string(),
                            )
                        }
                    }
                });
            }
            Err(error) => last_error = Some(error),
        }
    }

    Err(TransportError::RequestFailed(
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "custom DNS returned no websocket addresses".to_string()),
    ))
}

fn classify_websocket_error(
    operation: &str,
    token: &str,
    error: tungstenite::Error,
) -> TransportError {
    match error {
        tungstenite::Error::Http(response) => {
            let detail = response
                .body()
                .as_ref()
                .map(|body| String::from_utf8_lossy(body).to_string())
                .unwrap_or_default();
            classify_client_token_response(operation, token, response.status().as_u16(), &detail)
        }
        error => TransportError::RequestFailed(error.to_string()),
    }
}

fn classify_client_token_response(
    operation: &str,
    token: &str,
    status_code: u16,
    body: &str,
) -> TransportError {
    let detail = body.trim().to_string();
    if indicates_invalid_client_token_response(status_code, &detail) {
        return TransportError::InvalidClientToken {
            operation: operation.to_string(),
            token: token.trim().to_string(),
            status_code,
            detail,
        };
    }

    if detail.is_empty() {
        TransportError::RequestFailed(format!("status={status_code}"))
    } else {
        TransportError::RequestFailed(format!("status={status_code} {detail}"))
    }
}

fn indicates_invalid_client_token_response(status_code: u16, body: &str) -> bool {
    if status_code != 401 {
        return false;
    }

    let body = body.trim().to_ascii_lowercase();
    if body.is_empty() {
        return false;
    }

    body.contains("invalid token")
        || body.contains("token is required")
        || body.contains("failed to validate token")
}

fn token_from_request(request: &tungstenite::handshake::client::Request) -> String {
    request
        .uri()
        .query()
        .map(token_from_query)
        .unwrap_or_default()
}

fn token_from_url(url: &str) -> String {
    url.split_once('?')
        .map(|(_, query)| token_from_query(query))
        .unwrap_or_default()
}

fn token_from_query(query: &str) -> String {
    query
        .split('&')
        .filter_map(|part| part.split_once('='))
        .find_map(|(key, value)| (key == "token").then(|| percent_decode(value)))
        .unwrap_or_default()
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hi = hex_value(bytes[index + 1]);
            let lo = hex_value(bytes[index + 2]);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                output.push((hi << 4) | lo);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).to_string()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
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
