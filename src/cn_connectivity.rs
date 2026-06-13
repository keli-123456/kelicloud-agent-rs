use crate::ping::{LinuxPingExecutor, PingExecutor, PingTask};
use crate::protocol::BackendMessage;
use crate::report::{Report, ReportGenerator};
use crate::runtime::ControlMessageHandler;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const DEFAULT_INTERVAL_SECONDS: i32 = 60;
const DEFAULT_RETRY_ATTEMPTS: i32 = 3;
const DEFAULT_RETRY_DELAY_SECONDS: i32 = 1;
const DEFAULT_TIMEOUT_SECONDS: i32 = 5;
const FAILURE_LIMIT: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CnConnectivityProbeConfig {
    enabled: bool,
    targets: Vec<String>,
    targets_key: String,
    interval_seconds: i32,
    retry_attempts: i32,
    retry_delay_seconds: i32,
    timeout_seconds: i32,
}

impl Default for CnConnectivityProbeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            targets: Vec::new(),
            targets_key: String::new(),
            interval_seconds: DEFAULT_INTERVAL_SECONDS,
            retry_attempts: DEFAULT_RETRY_ATTEMPTS,
            retry_delay_seconds: DEFAULT_RETRY_DELAY_SECONDS,
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
        }
    }
}

impl CnConnectivityProbeConfig {
    fn from_backend(
        enabled: bool,
        target: Option<&str>,
        interval_seconds: i32,
        retry_attempts: i32,
        retry_delay_seconds: i32,
        timeout_seconds: i32,
    ) -> Self {
        let targets = parse_targets(target.unwrap_or_default());
        Self {
            enabled: enabled && !targets.is_empty(),
            targets_key: targets.join("\n"),
            targets,
            interval_seconds: positive_or_default(interval_seconds, DEFAULT_INTERVAL_SECONDS),
            retry_attempts: positive_or_default(retry_attempts, DEFAULT_RETRY_ATTEMPTS),
            retry_delay_seconds: positive_or_default(
                retry_delay_seconds,
                DEFAULT_RETRY_DELAY_SECONDS,
            ),
            timeout_seconds: positive_or_default(timeout_seconds, DEFAULT_TIMEOUT_SECONDS),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CnConnectivityProbeResult {
    pub status: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency: Option<i64>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consecutive_failures: Option<u32>,
}

#[derive(Debug, Clone)]
struct CnConnectivityInner {
    config: CnConnectivityProbeConfig,
    result: Option<CnConnectivityProbeResult>,
    failures: u32,
}

impl Default for CnConnectivityInner {
    fn default() -> Self {
        Self {
            config: CnConnectivityProbeConfig::default(),
            result: None,
            failures: 0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CnConnectivityState {
    inner: Arc<Mutex<CnConnectivityInner>>,
}

impl CnConnectivityState {
    pub fn current_report_value(&self) -> Option<serde_json::Value> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .result
            .as_ref()
            .and_then(|result| serde_json::to_value(result).ok())
    }

    pub fn update_config(
        &self,
        enabled: bool,
        target: Option<&str>,
        interval_seconds: i32,
        retry_attempts: i32,
        retry_delay_seconds: i32,
        timeout_seconds: i32,
    ) {
        let config = CnConnectivityProbeConfig::from_backend(
            enabled,
            target,
            interval_seconds,
            retry_attempts,
            retry_delay_seconds,
            timeout_seconds,
        );

        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.failures = 0;
        inner.result = if config.enabled {
            Some(CnConnectivityProbeResult {
                status: "unknown".to_string(),
                target: config.targets[0].clone(),
                latency: None,
                message: "waiting for probe".to_string(),
                checked_at: None,
                consecutive_failures: None,
            })
        } else {
            None
        };
        inner.config = config;
    }

    pub fn probe_once_with<P, W>(&self, prober: P, wait_retry: W)
    where
        P: FnMut(&str) -> Result<i64, String>,
        W: FnMut(),
    {
        let config = self.current_config();
        if !config.enabled {
            return;
        }

        let result = probe_targets_with_retries(&config, prober, wait_retry);
        self.store_probe_result(config, result);
    }

    pub fn probe_once(&self) {
        let config = self.current_config();
        if !config.enabled {
            return;
        }

        let retry_delay = Duration::from_secs(config.retry_delay_seconds.max(1) as u64);
        let timeout = Duration::from_secs(config.timeout_seconds.max(1) as u64);
        let executor = LinuxPingExecutor::new(timeout);
        let result = probe_targets_with_retries(
            &config,
            |target| {
                let task = PingTask {
                    task_id: 0,
                    ping_type: "icmp".to_string(),
                    target: target.to_string(),
                };
                let value = executor.run(&task).value;
                if value >= 0 {
                    Ok(i64::from(value))
                } else {
                    Err("connectivity probe failed".to_string())
                }
            },
            || thread::sleep(retry_delay),
        );
        self.store_probe_result(config, result);
    }

    pub fn start_probe_loop(&self) {
        let state = self.clone();
        thread::spawn(move || loop {
            let config = state.current_config();
            if config.enabled {
                state.probe_once();
                thread::sleep(Duration::from_secs(config.interval_seconds.max(1) as u64));
            } else {
                thread::sleep(Duration::from_secs(1));
            }
        });
    }

    fn current_config(&self) -> CnConnectivityProbeConfig {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .config
            .clone()
    }

    fn store_probe_result(
        &self,
        config: CnConnectivityProbeConfig,
        mut result: CnConnectivityProbeResult,
    ) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.config != config || !inner.config.enabled {
            return;
        }

        if result.status == "ok" {
            inner.failures = 0;
            result.consecutive_failures = None;
        } else {
            inner.failures += 1;
            result.status = if inner.failures >= FAILURE_LIMIT {
                "blocked_suspected".to_string()
            } else {
                "degraded".to_string()
            };
            result.consecutive_failures = Some(inner.failures);
        }

        inner.result = Some(result);
    }
}

#[derive(Debug, Clone)]
pub struct CnConnectivityControlMessageHandler {
    state: CnConnectivityState,
}

impl CnConnectivityControlMessageHandler {
    pub fn new(state: CnConnectivityState) -> Self {
        Self { state }
    }
}

impl ControlMessageHandler for CnConnectivityControlMessageHandler {
    fn handle(&mut self, message: BackendMessage) {
        if let BackendMessage::CnConnectivityProbeConfig {
            enabled,
            target,
            interval_seconds,
            retry_attempts,
            retry_delay_seconds,
            timeout_seconds,
        } = message
        {
            self.state.update_config(
                enabled,
                target.as_deref(),
                interval_seconds,
                retry_attempts,
                retry_delay_seconds,
                timeout_seconds,
            );
        }
    }
}

#[derive(Debug, Clone)]
pub struct CnConnectivityReportGenerator<G> {
    inner: G,
    state: CnConnectivityState,
}

impl<G> CnConnectivityReportGenerator<G> {
    pub fn new(inner: G, state: CnConnectivityState) -> Self {
        Self { inner, state }
    }
}

impl<G> ReportGenerator for CnConnectivityReportGenerator<G>
where
    G: ReportGenerator,
{
    fn generate(&self) -> Report {
        let mut report = self.inner.generate();
        report.cn_connectivity = self.state.current_report_value();
        report
    }
}

pub fn probe_target_with_retries<P, W>(
    target: &str,
    retry_attempts: i32,
    mut prober: P,
    mut wait_retry: W,
) -> CnConnectivityProbeResult
where
    P: FnMut(&str) -> Result<i64, String>,
    W: FnMut(),
{
    let attempts = positive_or_default(retry_attempts, DEFAULT_RETRY_ATTEMPTS);
    let mut last_error = None;

    for attempt in 1..=attempts {
        match prober(target) {
            Ok(latency) => {
                return CnConnectivityProbeResult {
                    status: "ok".to_string(),
                    target: target.to_string(),
                    latency: Some(latency),
                    message: "icmp reachable".to_string(),
                    checked_at: Some(chrono::Utc::now().to_rfc3339()),
                    consecutive_failures: None,
                };
            }
            Err(error) => {
                last_error = Some(error);
                if attempt < attempts {
                    wait_retry();
                }
            }
        }
    }

    let message = match last_error {
        Some(error) if attempts == 1 => error,
        Some(error) => format!("{error} after {attempts} attempts"),
        None => "connectivity probe failed".to_string(),
    };

    CnConnectivityProbeResult {
        status: "degraded".to_string(),
        target: target.to_string(),
        latency: None,
        message,
        checked_at: Some(chrono::Utc::now().to_rfc3339()),
        consecutive_failures: None,
    }
}

fn probe_targets_with_retries<P, W>(
    config: &CnConnectivityProbeConfig,
    mut prober: P,
    mut wait_retry: W,
) -> CnConnectivityProbeResult
where
    P: FnMut(&str) -> Result<i64, String>,
    W: FnMut(),
{
    if config.targets.is_empty() {
        return CnConnectivityProbeResult {
            status: "degraded".to_string(),
            target: String::new(),
            latency: None,
            message: "no targets configured".to_string(),
            checked_at: Some(chrono::Utc::now().to_rfc3339()),
            consecutive_failures: None,
        };
    }

    let mut failures = Vec::new();
    for target in &config.targets {
        let result = probe_target_with_retries(
            target,
            config.retry_attempts,
            |target| prober(target),
            || wait_retry(),
        );
        if result.status == "ok" {
            return result;
        }
        failures.push(format!("{target}: {}", result.message));
    }

    CnConnectivityProbeResult {
        status: "degraded".to_string(),
        target: config.targets.join(", "),
        latency: None,
        message: failures.join("; "),
        checked_at: Some(chrono::Utc::now().to_rfc3339()),
        consecutive_failures: None,
    }
}

fn parse_targets(value: &str) -> Vec<String> {
    let mut targets = Vec::new();
    for target in value.split(|ch| matches!(ch, '\n' | '\r' | ',' | ';')) {
        let target = target.trim();
        if !target.is_empty() && !targets.iter().any(|existing| existing == target) {
            targets.push(target.to_string());
        }
    }
    targets
}

fn positive_or_default(value: i32, default: i32) -> i32 {
    if value > 0 {
        value
    } else {
        default
    }
}
