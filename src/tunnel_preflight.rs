use std::net::{IpAddr, TcpListener};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TunnelTcpRulePreflightInput {
    pub rule_id: u64,
    pub listen_address: String,
    pub listen_port: u16,
    pub target_host: String,
    pub target_port: u16,
    pub source_allowlist: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TunnelPreflightIssue {
    pub rule_id: u64,
    pub code: TunnelPreflightIssueCode,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TunnelPreflightIssueCode {
    UnsupportedOs,
    ListenBindFailed,
    InvalidTarget,
    InvalidAllowlist,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TunnelPreflightSide {
    Ingress,
    Egress,
}

pub fn tunnel_supported_on_this_os() -> bool {
    cfg!(target_os = "linux")
}

pub fn validate_tunnel_tcp_rule(input: &TunnelTcpRulePreflightInput) -> Vec<TunnelPreflightIssue> {
    let mut issues = Vec::new();
    if !tunnel_supported_on_this_os() {
        issues.push(TunnelPreflightIssue {
            rule_id: input.rule_id,
            code: TunnelPreflightIssueCode::UnsupportedOs,
            message: "tunnel forwarding is only supported on Linux".to_string(),
        });
    }
    if input.target_host.trim().is_empty() || input.target_port == 0 {
        issues.push(TunnelPreflightIssue {
            rule_id: input.rule_id,
            code: TunnelPreflightIssueCode::InvalidTarget,
            message: "target host and port are required".to_string(),
        });
    }
    if !allowlist_is_valid(&input.source_allowlist) {
        issues.push(TunnelPreflightIssue {
            rule_id: input.rule_id,
            code: TunnelPreflightIssueCode::InvalidAllowlist,
            message: "source allowlist contains an invalid IP or CIDR entry".to_string(),
        });
    }
    if let Some(issue) = check_listener_bindable(&input.listen_address, input.listen_port) {
        issues.push(TunnelPreflightIssue {
            rule_id: input.rule_id,
            ..issue
        });
    }
    issues
}

pub fn validate_tunnel_tcp_rule_for_side(
    input: &TunnelTcpRulePreflightInput,
    side: TunnelPreflightSide,
) -> Vec<TunnelPreflightIssue> {
    validate_tunnel_tcp_rule_for_side_with_os(input, side, tunnel_supported_on_this_os())
}

pub fn validate_tunnel_tcp_rule_for_side_with_os(
    input: &TunnelTcpRulePreflightInput,
    side: TunnelPreflightSide,
    os_supported: bool,
) -> Vec<TunnelPreflightIssue> {
    let mut issues = Vec::new();
    if !os_supported {
        issues.push(TunnelPreflightIssue {
            rule_id: input.rule_id,
            code: TunnelPreflightIssueCode::UnsupportedOs,
            message: "tunnel forwarding is only supported on Linux".to_string(),
        });
    }

    match side {
        TunnelPreflightSide::Ingress => {
            if !allowlist_is_valid(&input.source_allowlist) {
                issues.push(TunnelPreflightIssue {
                    rule_id: input.rule_id,
                    code: TunnelPreflightIssueCode::InvalidAllowlist,
                    message: "source allowlist contains an invalid IP or CIDR entry".to_string(),
                });
            }
            if let Some(issue) = check_listener_bindable(&input.listen_address, input.listen_port) {
                issues.push(TunnelPreflightIssue {
                    rule_id: input.rule_id,
                    ..issue
                });
            }
        }
        TunnelPreflightSide::Egress => {
            if input.target_host.trim().is_empty() || input.target_port == 0 {
                issues.push(TunnelPreflightIssue {
                    rule_id: input.rule_id,
                    code: TunnelPreflightIssueCode::InvalidTarget,
                    message: "target host and port are required".to_string(),
                });
            }
        }
    }
    issues
}

pub fn tunnel_preflight_status(code: TunnelPreflightIssueCode) -> &'static str {
    match code {
        TunnelPreflightIssueCode::UnsupportedOs => "unsupported_os",
        TunnelPreflightIssueCode::ListenBindFailed => "listen_bind_failed",
        TunnelPreflightIssueCode::InvalidTarget => "invalid_target",
        TunnelPreflightIssueCode::InvalidAllowlist => "invalid_allowlist",
    }
}

pub fn check_listener_bindable(
    listen_address: &str,
    listen_port: u16,
) -> Option<TunnelPreflightIssue> {
    let endpoint = tcp_endpoint(listen_address, listen_port);
    match TcpListener::bind(&endpoint) {
        Ok(listener) => {
            drop(listener);
            None
        }
        Err(error) => Some(TunnelPreflightIssue {
            rule_id: 0,
            code: TunnelPreflightIssueCode::ListenBindFailed,
            message: format!("cannot bind listener {endpoint}: {error}"),
        }),
    }
}

pub fn allowlist_is_valid(allowlist: &str) -> bool {
    let allowlist = allowlist.trim();
    if allowlist.is_empty() {
        return true;
    }
    allowlist
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .filter(|entry| !entry.trim().is_empty())
        .all(allowlist_entry_is_valid)
}

pub fn allowlist_entry_is_valid(entry: &str) -> bool {
    if entry == "*" {
        return true;
    }
    if let Some((network, prefix)) = entry.split_once('/') {
        let Ok(ip) = network.trim().parse::<IpAddr>() else {
            return false;
        };
        let Ok(prefix_len) = prefix.trim().parse::<u8>() else {
            return false;
        };
        return match ip {
            IpAddr::V4(_) => prefix_len <= 32,
            IpAddr::V6(_) => prefix_len <= 128,
        };
    }
    entry.parse::<IpAddr>().is_ok()
}

pub fn tcp_endpoint(host: &str, port: u16) -> String {
    let host = host.trim();
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}
