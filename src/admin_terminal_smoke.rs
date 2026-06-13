use std::error::Error;
use std::fmt;
use std::net::{IpAddr, TcpStream};
use std::time::{Duration, Instant};

use tungstenite::client::IntoClientRequest;
use tungstenite::http::header::COOKIE;
use tungstenite::http::HeaderValue;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminTerminalSmokeError {
    EmptyEndpoint,
    EmptyClientUuid,
    EmptySessionToken,
    EmptyExpectedText,
    UnsupportedScheme(String),
    InvalidRequest(String),
    RequestFailed(String),
    TimedOut(String),
}

impl fmt::Display for AdminTerminalSmokeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyEndpoint => write!(f, "endpoint is required"),
            Self::EmptyClientUuid => write!(f, "client uuid is required"),
            Self::EmptySessionToken => write!(f, "session token is required"),
            Self::EmptyExpectedText => write!(f, "expected terminal text is required"),
            Self::UnsupportedScheme(scheme) => {
                write!(f, "unsupported endpoint scheme: {scheme}")
            }
            Self::InvalidRequest(message) => write!(f, "invalid terminal request: {message}"),
            Self::RequestFailed(message) => write!(f, "terminal request failed: {message}"),
            Self::TimedOut(message) => write!(f, "terminal smoke timed out: {message}"),
        }
    }
}

impl Error for AdminTerminalSmokeError {}

#[derive(Debug, Clone)]
pub struct AdminTerminalSmokeRequest {
    pub endpoint: String,
    pub session_token: String,
    pub client_uuid: String,
    pub command: String,
    pub expect: String,
    pub timeout: Duration,
}

pub fn build_admin_terminal_ws_url(
    endpoint: &str,
    client_uuid: &str,
) -> Result<String, AdminTerminalSmokeError> {
    let client_uuid = require_non_empty(client_uuid, AdminTerminalSmokeError::EmptyClientUuid)?;
    let base = normalize_ws_base(endpoint)?;
    Ok(format!(
        "{base}/api/admin/client/{}/terminal",
        percent_encode(client_uuid)
    ))
}

pub fn session_cookie_header(session_token: &str) -> String {
    format!("session_token={}", session_token.trim())
}

pub fn run_admin_terminal_smoke(
    request: &AdminTerminalSmokeRequest,
) -> Result<(), AdminTerminalSmokeError> {
    require_non_empty(
        &request.session_token,
        AdminTerminalSmokeError::EmptySessionToken,
    )?;
    require_non_empty(&request.expect, AdminTerminalSmokeError::EmptyExpectedText)?;

    let url = build_admin_terminal_ws_url(&request.endpoint, &request.client_uuid)?;
    let mut ws_request = url
        .into_client_request()
        .map_err(|error| AdminTerminalSmokeError::InvalidRequest(error.to_string()))?;
    let cookie = HeaderValue::from_str(&session_cookie_header(&request.session_token))
        .map_err(|error| AdminTerminalSmokeError::InvalidRequest(error.to_string()))?;
    ws_request.headers_mut().insert(COOKIE, cookie);

    let (mut socket, _response) = connect(ws_request)
        .map_err(|error| AdminTerminalSmokeError::RequestFailed(error.to_string()))?;
    set_read_timeout(&mut socket, Some(Duration::from_millis(250)))?;

    let command = normalize_terminal_command(&request.command);
    socket
        .send(Message::Text(command.into()))
        .map_err(|error| AdminTerminalSmokeError::RequestFailed(error.to_string()))?;

    let mut output = String::new();
    let deadline = Instant::now() + request.timeout;
    while Instant::now() < deadline {
        match socket.read() {
            Ok(Message::Text(text)) => output.push_str(&text),
            Ok(Message::Binary(bytes)) => output.push_str(&String::from_utf8_lossy(&bytes)),
            Ok(Message::Ping(bytes)) => {
                let _ = socket.send(Message::Pong(bytes));
            }
            Ok(Message::Close(_)) => {
                return Err(AdminTerminalSmokeError::RequestFailed(
                    "terminal websocket closed before expected output".to_string(),
                ));
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(error) => {
                return Err(AdminTerminalSmokeError::RequestFailed(error.to_string()));
            }
        }

        if output.contains(&request.expect) {
            let _ = socket.close(None);
            return Ok(());
        }
    }

    Err(AdminTerminalSmokeError::TimedOut(format!(
        "expected {:?}, last output {:?}",
        request.expect,
        tail(&output, 400)
    )))
}

fn normalize_terminal_command(command: &str) -> String {
    let trimmed = command.trim_end_matches(['\r', '\n']);
    format!("{trimmed}\n")
}

fn set_read_timeout(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    timeout: Option<Duration>,
) -> Result<(), AdminTerminalSmokeError> {
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => stream.set_read_timeout(timeout),
        MaybeTlsStream::Rustls(stream) => stream.sock.set_read_timeout(timeout),
        _ => Ok(()),
    }
    .map_err(|error| AdminTerminalSmokeError::RequestFailed(error.to_string()))
}

fn normalize_ws_base(endpoint: &str) -> Result<String, AdminTerminalSmokeError> {
    let endpoint = endpoint.trim().trim_end_matches('/');
    if endpoint.is_empty() {
        return Err(AdminTerminalSmokeError::EmptyEndpoint);
    }

    if let Some(rest) = endpoint.strip_prefix("https://") {
        return Ok(format!("wss://{}", endpoint_rest_with_ascii_host(rest)));
    }
    if let Some(rest) = endpoint.strip_prefix("http://") {
        return Ok(format!("ws://{}", endpoint_rest_with_ascii_host(rest)));
    }
    if let Some(rest) = endpoint.strip_prefix("wss://") {
        return Ok(format!("wss://{}", endpoint_rest_with_ascii_host(rest)));
    }
    if let Some(rest) = endpoint.strip_prefix("ws://") {
        return Ok(format!("ws://{}", endpoint_rest_with_ascii_host(rest)));
    }

    let scheme = endpoint
        .split_once("://")
        .map(|(scheme, _)| scheme.to_string())
        .unwrap_or_else(|| "missing".to_string());
    Err(AdminTerminalSmokeError::UnsupportedScheme(scheme))
}

fn endpoint_rest_with_ascii_host(rest: &str) -> String {
    let (authority, suffix) = rest
        .split_once('/')
        .map(|(authority, suffix)| (authority, format!("/{suffix}")))
        .unwrap_or_else(|| (rest, String::new()));
    format!("{}{}", authority_with_ascii_host(authority), suffix)
}

fn authority_with_ascii_host(authority: &str) -> String {
    if authority.starts_with('[') {
        return authority.to_string();
    }

    let colon_count = authority.matches(':').count();
    let (host, port) = if colon_count == 1 {
        authority
            .split_once(':')
            .map(|(host, port)| (host, format!(":{port}")))
            .unwrap_or((authority, String::new()))
    } else {
        (authority, String::new())
    };

    if host.parse::<IpAddr>().is_ok() || host.is_empty() {
        return authority.to_string();
    }

    let ascii_host = idna::domain_to_ascii(host).unwrap_or_else(|_| host.to_string());
    format!("{ascii_host}{port}")
}

fn require_non_empty(
    value: &str,
    error: AdminTerminalSmokeError,
) -> Result<&str, AdminTerminalSmokeError> {
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

fn tail(value: &str, max_chars: usize) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    let start = chars.len().saturating_sub(max_chars);
    chars[start..].iter().collect()
}
