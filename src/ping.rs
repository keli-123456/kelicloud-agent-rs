use std::net::{IpAddr, TcpStream, ToSocketAddrs};
use std::process::Command;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingTask {
    pub task_id: u32,
    pub ping_type: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PingResult {
    #[serde(rename = "type")]
    pub message_type: String,
    pub task_id: u32,
    pub ping_type: String,
    pub value: i32,
    pub finished_at: String,
}

impl PingResult {
    pub fn new(
        task_id: u32,
        ping_type: impl Into<String>,
        value: i32,
        finished_at: impl Into<String>,
    ) -> Self {
        Self {
            message_type: "ping_result".to_string(),
            task_id,
            ping_type: ping_type.into(),
            value,
            finished_at: finished_at.into(),
        }
    }

    pub fn now(task: &PingTask, value: i32) -> Self {
        Self::new(
            task.task_id,
            task.ping_type.clone(),
            value,
            chrono::Utc::now().to_rfc3339(),
        )
    }
}

pub trait PingExecutor {
    fn run(&self, task: &PingTask) -> PingResult;
}

#[derive(Debug, Clone, Copy)]
pub struct FixedPingExecutor {
    value: i32,
}

impl FixedPingExecutor {
    pub fn new(value: i32) -> Self {
        Self { value }
    }
}

impl PingExecutor for FixedPingExecutor {
    fn run(&self, task: &PingTask) -> PingResult {
        PingResult::now(task, self.value)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopPingExecutor;

impl PingExecutor for NoopPingExecutor {
    fn run(&self, task: &PingTask) -> PingResult {
        PingResult::now(task, -1)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LinuxPingExecutor {
    timeout: Duration,
}

impl LinuxPingExecutor {
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl Default for LinuxPingExecutor {
    fn default() -> Self {
        Self::new(Duration::from_secs(3))
    }
}

impl PingExecutor for LinuxPingExecutor {
    fn run(&self, task: &PingTask) -> PingResult {
        let value = match task.ping_type.as_str() {
            "icmp" => icmp_ping(&task.target, self.timeout),
            "tcp" => tcp_ping(&task.target, self.timeout),
            "http" => http_ping(&task.target, self.timeout),
            _ => None,
        }
        .map(|latency| latency.min(i32::MAX as u128) as i32)
        .unwrap_or(-1);

        PingResult::now(task, value)
    }
}

pub fn parse_linux_ping_latency_ms(output: &str) -> Option<i32> {
    let (_, after) = output.split_once("time=")?;
    let raw = after.split_whitespace().next()?;
    let value = raw.parse::<f64>().ok()?;
    Some(value.round() as i32)
}

fn icmp_ping(target: &str, timeout: Duration) -> Option<u128> {
    let host = host_without_port(target);
    let timeout_seconds = timeout.as_secs().max(1).to_string();
    let output = Command::new("ping")
        .args(["-c", "1", "-W", &timeout_seconds, &host])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_linux_ping_latency_ms(&stdout).map(|value| value.max(0) as u128)
}

fn tcp_ping(target: &str, timeout: Duration) -> Option<u128> {
    let address = socket_address_or_default(target, "80")?;
    let start = Instant::now();
    let stream = TcpStream::connect_timeout(&address, timeout).ok()?;
    drop(stream);
    Some(start.elapsed().as_millis())
}

fn http_ping(target: &str, timeout: Duration) -> Option<u128> {
    let url = if target.starts_with("http://") || target.starts_with("https://") {
        target.to_string()
    } else {
        format!("http://{target}")
    };
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()
        .ok()?;
    let start = Instant::now();
    let response = client.get(url).send().ok()?;
    if response.status().as_u16() < 400 {
        Some(start.elapsed().as_millis())
    } else {
        None
    }
}

fn socket_address_or_default(target: &str, default_port: &str) -> Option<std::net::SocketAddr> {
    let candidate = normalize_socket_target(target, default_port)?;
    candidate.to_socket_addrs().ok()?.next()
}

fn host_without_port(target: &str) -> String {
    let target = target.trim();
    if let Some(host) = bracketed_host(target) {
        return host.to_string();
    }
    if target.parse::<IpAddr>().is_ok() {
        return target.to_string();
    }
    if let Some((host, _)) = split_explicit_host_port(target) {
        return host.trim_matches(['[', ']']).to_string();
    }
    target.trim_matches(['[', ']']).to_string()
}

fn normalize_socket_target(target: &str, default_port: &str) -> Option<String> {
    let target = target.trim();
    if target.is_empty() {
        return None;
    }

    if let Some(host) = bracketed_host(target) {
        if split_explicit_host_port(target).is_some() {
            return Some(target.to_string());
        }
        return Some(format!("[{host}]:{default_port}"));
    }

    if let Ok(address) = target.parse::<IpAddr>() {
        return if address.is_ipv6() {
            Some(format!("[{target}]:{default_port}"))
        } else {
            Some(format!("{target}:{default_port}"))
        };
    }

    if split_explicit_host_port(target).is_some() {
        return Some(target.to_string());
    }

    Some(format!("{target}:{default_port}"))
}

fn bracketed_host(target: &str) -> Option<&str> {
    let rest = target.strip_prefix('[')?;
    let (host, _) = rest.split_once(']')?;
    if host.is_empty() {
        return None;
    }
    Some(host)
}

fn split_explicit_host_port(target: &str) -> Option<(&str, &str)> {
    let (host, port) = target.rsplit_once(':')?;
    if host.is_empty() || port.is_empty() || !port.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    if target.matches(':').count() == 1 || host.starts_with('[') {
        return Some((host, port));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{host_without_port, socket_address_or_default};

    #[test]
    fn socket_address_adds_default_port_to_ipv6_literal() {
        let address = socket_address_or_default("2607:f358:1a:e::ab0:39b7", "80").unwrap();

        assert_eq!(address.to_string(), "[2607:f358:1a:e::ab0:39b7]:80");
    }

    #[test]
    fn host_without_port_preserves_ipv6_literal() {
        assert_eq!(
            host_without_port("2607:f358:1a:e::ab0:39b7"),
            "2607:f358:1a:e::ab0:39b7"
        );
    }
}
