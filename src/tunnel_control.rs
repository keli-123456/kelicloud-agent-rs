use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;

use crate::transport::{HeaderPair, TransportError};

pub const TUNNEL_CONTROL_PROTOCOL_V1: &str = "keli-tunnel-control.v1";

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
        data_plane: false,
    }
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
    let mut socket = match transport.connect_tunnel_control(url, headers) {
        Ok(socket) => socket,
        Err(error) if is_non_fatal_tunnel_control_error(&error) => return Ok(()),
        Err(error) => return Err(error),
    };

    socket.send_message(&build_hello(agent_version))?;
    let mut latest_revision = String::new();
    let mut accepted_rules = Vec::new();

    while let Some(bytes) = socket.read_message()? {
        match parse_server_message(&bytes) {
            Ok(TunnelControlServerMessage::HelloAck {
                server_protocol, ..
            }) => {
                if server_protocol.trim() != TUNNEL_CONTROL_PROTOCOL_V1 {
                    return Err(TransportError::RequestFailed(format!(
                        "unsupported tunnel control protocol: {server_protocol}"
                    )));
                }
            }
            Ok(TunnelControlServerMessage::RuleSync { revision, rules }) => {
                latest_revision = revision;
                accepted_rules = rules;
                socket.send_message(&build_rule_ack(&latest_revision, &accepted_rules, &[]))?;
                let statuses = accepted_rules
                    .iter()
                    .map(|rule| TunnelRuleStatus {
                        id: rule.id,
                        status: "ok".to_string(),
                        error: String::new(),
                    })
                    .collect::<Vec<_>>();
                socket.send_message(&build_rule_status(&latest_revision, &statuses))?;
            }
            Ok(TunnelControlServerMessage::Error { code, message }) => {
                if code == "feature_disabled" {
                    return Ok(());
                }
                return Err(TransportError::RequestFailed(message));
            }
            Err(error) => return Err(TransportError::RequestFailed(error.to_string())),
        }
    }

    let active_rules = accepted_rules
        .iter()
        .map(|rule| rule.id)
        .collect::<Vec<_>>();
    socket.send_message(&build_heartbeat(&latest_revision, &active_rules))?;
    Ok(())
}
