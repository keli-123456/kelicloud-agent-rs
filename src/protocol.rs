use std::error::Error;
use std::fmt;
use std::net::IpAddr;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    EmptyEndpoint,
    EmptyToken,
    EmptyTerminalId,
    InvalidMessage(String),
    UnsupportedScheme(String),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyEndpoint => write!(f, "endpoint is required"),
            Self::EmptyToken => write!(f, "token is required"),
            Self::EmptyTerminalId => write!(f, "terminal id is required"),
            Self::InvalidMessage(error) => write!(f, "invalid backend message: {error}"),
            Self::UnsupportedScheme(scheme) => {
                write!(f, "unsupported endpoint scheme: {scheme}")
            }
        }
    }
}

impl Error for ProtocolError {}

pub fn build_report_ws_url(endpoint: &str, token: &str) -> Result<String, ProtocolError> {
    let token = require_non_empty(token, ProtocolError::EmptyToken)?;
    build_ws_url(endpoint, "/api/clients/report", &[("token", token)])
}

pub fn build_tunnel_control_ws_url(endpoint: &str, token: &str) -> Result<String, ProtocolError> {
    let token = require_non_empty(token, ProtocolError::EmptyToken)?;
    build_ws_url(endpoint, "/api/clients/tunnel", &[("token", token)])
}

pub fn build_tunnel_data_ws_url(endpoint: &str, token: &str) -> Result<String, ProtocolError> {
    let token = require_non_empty(token, ProtocolError::EmptyToken)?;
    build_ws_url(endpoint, "/api/clients/tunnel/data", &[("token", token)])
}

pub fn build_tunnel_data_ktp_tcp_url(address: &str) -> Result<String, ProtocolError> {
    let address = require_non_empty(address, ProtocolError::EmptyEndpoint)?;
    if address.starts_with("ktp+tcp://") {
        return Ok(address.to_string());
    }
    if address.starts_with("tcp://") {
        return Ok(format!("ktp+tcp://{}", &address["tcp://".len()..]));
    }
    Ok(format!("ktp+tcp://{address}"))
}

pub fn build_terminal_ws_url(
    endpoint: &str,
    token: &str,
    terminal_id: &str,
) -> Result<String, ProtocolError> {
    let token = require_non_empty(token, ProtocolError::EmptyToken)?;
    let terminal_id = require_non_empty(terminal_id, ProtocolError::EmptyTerminalId)?;
    build_ws_url(
        endpoint,
        "/api/clients/terminal",
        &[("token", token), ("id", terminal_id)],
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendMessage {
    CnConnectivityProbeConfig {
        enabled: bool,
        target: Option<String>,
        interval_seconds: i32,
        retry_attempts: i32,
        retry_delay_seconds: i32,
        timeout_seconds: i32,
    },
    Terminal {
        request_id: String,
    },
    Exec {
        task_id: String,
        command: String,
    },
    Ping {
        task_id: u32,
        ping_type: String,
        target: String,
    },
    Unknown {
        message: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
struct RawBackendMessage {
    message: Option<String>,
    request_id: Option<String>,
    command: Option<String>,
    task_id: Option<String>,
    ping_task_id: Option<u32>,
    ping_type: Option<String>,
    ping_target: Option<String>,
    cn_connectivity_enabled: Option<bool>,
    cn_connectivity_target: Option<String>,
    cn_connectivity_interval: Option<i32>,
    cn_connectivity_retry_attempts: Option<i32>,
    cn_connectivity_retry_delay_seconds: Option<i32>,
    cn_connectivity_timeout_seconds: Option<i32>,
}

pub fn parse_backend_message(bytes: &[u8]) -> Result<BackendMessage, ProtocolError> {
    let raw: RawBackendMessage = serde_json::from_slice(bytes)
        .map_err(|error| ProtocolError::InvalidMessage(error.to_string()))?;
    let message = raw
        .message
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if message == Some("cn_connectivity_probe_config") {
        return Ok(BackendMessage::CnConnectivityProbeConfig {
            enabled: raw.cn_connectivity_enabled.unwrap_or(false),
            target: raw
                .cn_connectivity_target
                .and_then(|target| non_empty_string(&target)),
            interval_seconds: raw.cn_connectivity_interval.unwrap_or(60),
            retry_attempts: raw.cn_connectivity_retry_attempts.unwrap_or(3),
            retry_delay_seconds: raw.cn_connectivity_retry_delay_seconds.unwrap_or(1),
            timeout_seconds: raw.cn_connectivity_timeout_seconds.unwrap_or(5),
        });
    }

    if message == Some("terminal")
        || raw
            .request_id
            .as_deref()
            .is_some_and(|id| !id.trim().is_empty())
    {
        if let Some(request_id) = raw.request_id.and_then(|id| non_empty_string(&id)) {
            return Ok(BackendMessage::Terminal { request_id });
        }
    }

    if message == Some("exec") {
        return Ok(BackendMessage::Exec {
            task_id: raw
                .task_id
                .and_then(|id| non_empty_string(&id))
                .unwrap_or_default(),
            command: raw
                .command
                .and_then(|command| non_empty_string(&command))
                .unwrap_or_default(),
        });
    }

    if message == Some("ping")
        || raw.ping_task_id.unwrap_or(0) != 0
        || raw
            .ping_type
            .as_deref()
            .is_some_and(|kind| !kind.trim().is_empty())
        || raw
            .ping_target
            .as_deref()
            .is_some_and(|target| !target.trim().is_empty())
    {
        return Ok(BackendMessage::Ping {
            task_id: raw.ping_task_id.unwrap_or(0),
            ping_type: raw
                .ping_type
                .and_then(|kind| non_empty_string(&kind))
                .unwrap_or_default(),
            target: raw
                .ping_target
                .and_then(|target| non_empty_string(&target))
                .unwrap_or_default(),
        });
    }

    Ok(BackendMessage::Unknown {
        message: message.map(str::to_string),
    })
}

fn build_ws_url(
    endpoint: &str,
    path: &str,
    query: &[(&str, &str)],
) -> Result<String, ProtocolError> {
    let base = normalize_ws_base(endpoint)?;
    let query = query
        .iter()
        .map(|(key, value)| format!("{key}={}", percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    Ok(format!("{base}{path}?{query}"))
}

fn non_empty_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn normalize_ws_base(endpoint: &str) -> Result<String, ProtocolError> {
    let endpoint = endpoint.trim().trim_end_matches('/');
    if endpoint.is_empty() {
        return Err(ProtocolError::EmptyEndpoint);
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
    Err(ProtocolError::UnsupportedScheme(scheme))
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

fn require_non_empty(value: &str, error: ProtocolError) -> Result<&str, ProtocolError> {
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
