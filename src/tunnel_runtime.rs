use crate::ktp::KtpFrame;
use crate::transport::TransportError;
use crate::tunnel_control::{SelectedTunnelRule, TunnelRuleStateSink};
use crate::tunnel_data::{TunnelDataReadySource, TunnelDataReadyState};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct SharedTunnelRuleState {
    inner: Arc<Mutex<TunnelRuleSnapshot>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TunnelRuleSnapshot {
    pub revision: String,
    pub rules: Vec<SelectedTunnelRule>,
}

impl SharedTunnelRuleState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(TunnelRuleSnapshot {
                revision: String::new(),
                rules: Vec::new(),
            })),
        }
    }

    pub fn snapshot(&self) -> TunnelRuleSnapshot {
        self.inner
            .lock()
            .map(|state| state.clone())
            .unwrap_or_else(|_| TunnelRuleSnapshot {
                revision: String::new(),
                rules: Vec::new(),
            })
    }

    pub fn tcp_listener_plan(&self) -> Vec<TunnelTcpListenerSpec> {
        build_tcp_listener_plan(&self.snapshot().rules)
    }
}

impl Default for SharedTunnelRuleState {
    fn default() -> Self {
        Self::new()
    }
}

impl TunnelRuleStateSink for SharedTunnelRuleState {
    fn update_rules(&self, revision: &str, rules: &[SelectedTunnelRule]) {
        if let Ok(mut state) = self.inner.lock() {
            state.revision = revision.trim().to_string();
            state.rules = rules.to_vec();
        }
    }
}

impl TunnelDataReadySource for SharedTunnelRuleState {
    fn current_ready(&self) -> TunnelDataReadyState {
        let snapshot = self.snapshot();
        TunnelDataReadyState::from_selected_rules(&snapshot.revision, &snapshot.rules)
    }
}

pub trait TunnelSessionRuntime {
    fn handle_server_frame(&mut self, _frame: KtpFrame) -> Result<Vec<KtpFrame>, TransportError> {
        Ok(Vec::new())
    }

    fn next_client_frame(&mut self) -> Result<Option<KtpFrame>, TransportError> {
        Ok(None)
    }
}

#[derive(Debug, Default)]
pub struct NoopTunnelSessionRuntime;

impl TunnelSessionRuntime for NoopTunnelSessionRuntime {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TunnelTcpListenerSpec {
    pub rule_id: u64,
    pub name: String,
    pub listen_address: String,
    pub listen_port: u16,
    pub target_host: String,
    pub target_port: u16,
    pub source_allowlist: String,
    pub max_concurrent_sessions: u32,
}

pub fn build_tcp_listener_plan(rules: &[SelectedTunnelRule]) -> Vec<TunnelTcpListenerSpec> {
    let mut listeners = rules
        .iter()
        .filter(|rule| rule.enabled)
        .filter(|rule| rule.protocol.trim().eq_ignore_ascii_case("tcp"))
        .filter(|rule| matches!(rule.role.trim(), "ingress" | "both"))
        .map(|rule| TunnelTcpListenerSpec {
            rule_id: rule.id,
            name: rule.name.trim().to_string(),
            listen_address: rule.listen_address.trim().to_string(),
            listen_port: rule.listen_port,
            target_host: rule.target_host.trim().to_string(),
            target_port: rule.target_port,
            source_allowlist: rule.source_allowlist.trim().to_string(),
            max_concurrent_sessions: rule.max_concurrent_sessions,
        })
        .collect::<Vec<_>>();
    listeners.sort_by_key(|listener| listener.rule_id);
    listeners
}

pub fn source_addr_allowed(source_addr: &str, allowlist: &str) -> bool {
    let allowlist = allowlist.trim();
    if allowlist.is_empty() {
        return true;
    }
    let Some(source_ip) = parse_source_ip(source_addr) else {
        return false;
    };
    allowlist
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .filter(|entry| !entry.trim().is_empty())
        .any(|entry| source_ip_matches_entry(source_ip, entry.trim()))
}

fn parse_source_ip(source_addr: &str) -> Option<IpAddr> {
    let source_addr = source_addr.trim();
    source_addr
        .parse::<SocketAddr>()
        .map(|addr| addr.ip())
        .ok()
        .or_else(|| source_addr.parse::<IpAddr>().ok())
}

fn source_ip_matches_entry(source_ip: IpAddr, entry: &str) -> bool {
    if entry == "*" {
        return true;
    }
    if let Some((network, prefix)) = entry.split_once('/') {
        let Ok(network_ip) = network.trim().parse::<IpAddr>() else {
            return false;
        };
        let Ok(prefix_len) = prefix.trim().parse::<u8>() else {
            return false;
        };
        return ip_in_cidr(source_ip, network_ip, prefix_len);
    }
    entry
        .parse::<IpAddr>()
        .map(|allowed_ip| allowed_ip == source_ip)
        .unwrap_or(false)
}

fn ip_in_cidr(source_ip: IpAddr, network_ip: IpAddr, prefix_len: u8) -> bool {
    match (source_ip, network_ip) {
        (IpAddr::V4(source), IpAddr::V4(network)) if prefix_len <= 32 => {
            let mask = prefix_mask_u32(prefix_len);
            (u32::from(source) & mask) == (u32::from(network) & mask)
        }
        (IpAddr::V6(source), IpAddr::V6(network)) if prefix_len <= 128 => {
            let mask = prefix_mask_u128(prefix_len);
            (u128::from(source) & mask) == (u128::from(network) & mask)
        }
        _ => false,
    }
}

fn prefix_mask_u32(prefix_len: u8) -> u32 {
    if prefix_len == 0 {
        0
    } else {
        u32::MAX << (32 - prefix_len)
    }
}

fn prefix_mask_u128(prefix_len: u8) -> u128 {
    if prefix_len == 0 {
        0
    } else {
        u128::MAX << (128 - prefix_len)
    }
}
