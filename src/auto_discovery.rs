use crate::config::AgentConfig;
use crate::transport::{access_headers, HeaderPair};
use serde::{Deserialize, Serialize};
use std::env;
use std::error::Error;
use std::fmt;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoDiscoveryCache {
    pub uuid: String,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoDiscoveryRegisterRequest {
    pub url: String,
    pub key: String,
    pub headers: Vec<HeaderPair>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoDiscoveryError {
    EmptyEndpoint,
    EmptyKey,
    UnsupportedScheme(String),
    CachePath(String),
    CacheWrite(String),
    RequestFailed(String),
    RegisterRejected(String),
    EmptyRegisteredToken,
}

impl fmt::Display for AutoDiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyEndpoint => write!(f, "endpoint is required"),
            Self::EmptyKey => write!(f, "auto-discovery key is required"),
            Self::UnsupportedScheme(scheme) => write!(f, "unsupported endpoint scheme: {scheme}"),
            Self::CachePath(message) => {
                write!(f, "failed to resolve auto-discovery cache path: {message}")
            }
            Self::CacheWrite(message) => {
                write!(f, "failed to write auto-discovery cache: {message}")
            }
            Self::RequestFailed(message) => write!(f, "auto-discovery request failed: {message}"),
            Self::RegisterRejected(message) => {
                write!(f, "auto-discovery register rejected: {message}")
            }
            Self::EmptyRegisteredToken => {
                write!(f, "auto-discovery response did not include a token")
            }
        }
    }
}

impl Error for AutoDiscoveryError {}

pub trait AutoDiscoveryRegistrar {
    fn register(
        &mut self,
        request: AutoDiscoveryRegisterRequest,
    ) -> Result<AutoDiscoveryCache, AutoDiscoveryError>;
}

pub fn resolve_auto_discovery(config: &mut AgentConfig) -> Result<bool, AutoDiscoveryError> {
    if config.auto_discovery_key.trim().is_empty() {
        return Ok(false);
    }

    let cache_path = default_auto_discovery_cache_path()?;
    let hostname = system_hostname();
    let mut registrar = ReqwestAutoDiscoveryRegistrar::from_config(config)?;
    resolve_auto_discovery_with(config, &cache_path, &hostname, &mut registrar)
}

pub fn resolve_auto_discovery_with<R>(
    config: &mut AgentConfig,
    cache_path: &Path,
    hostname: &str,
    registrar: &mut R,
) -> Result<bool, AutoDiscoveryError>
where
    R: AutoDiscoveryRegistrar,
{
    let key = config.auto_discovery_key.trim();
    if key.is_empty() {
        return Ok(false);
    }

    if let Some(cache) = read_cache(cache_path) {
        config.token = cache.token;
        return Ok(false);
    }

    let request = AutoDiscoveryRegisterRequest {
        url: build_auto_discovery_register_url(&config.endpoint, hostname)?,
        key: key.to_string(),
        headers: access_headers(config),
    };
    let cache = registrar.register(request)?;
    if cache.token.trim().is_empty() {
        return Err(AutoDiscoveryError::EmptyRegisteredToken);
    }
    save_cache(cache_path, &cache)?;
    config.token = cache.token;
    Ok(true)
}

pub fn build_auto_discovery_register_url(
    endpoint: &str,
    hostname: &str,
) -> Result<String, AutoDiscoveryError> {
    let endpoint = normalize_http_base(endpoint)?;
    Ok(format!(
        "{endpoint}/api/clients/register?name={}",
        percent_encode(hostname)
    ))
}

pub fn default_auto_discovery_cache_path() -> Result<PathBuf, AutoDiscoveryError> {
    let exe =
        env::current_exe().map_err(|error| AutoDiscoveryError::CachePath(error.to_string()))?;
    let dir = exe
        .parent()
        .ok_or_else(|| AutoDiscoveryError::CachePath("current executable has no parent".into()))?;
    Ok(dir.join("auto-discovery.json"))
}

pub fn system_hostname() -> String {
    env::var("HOSTNAME")
        .ok()
        .and_then(|value| non_empty_string(&value))
        .or_else(|| read_hostname_file("/proc/sys/kernel/hostname"))
        .or_else(|| read_hostname_file("/etc/hostname"))
        .unwrap_or_else(|| "localhost".to_string())
}

pub struct ReqwestAutoDiscoveryRegistrar {
    client: reqwest::blocking::Client,
}

impl ReqwestAutoDiscoveryRegistrar {
    pub fn from_config(config: &AgentConfig) -> Result<Self, AutoDiscoveryError> {
        let mut builder =
            reqwest::blocking::Client::builder().danger_accept_invalid_certs(config.insecure);
        let custom_dns = config.custom_dns.trim();
        if !custom_dns.is_empty() {
            builder = builder.dns_resolver(Arc::new(crate::linux_proc::CustomDnsResolver::new(
                custom_dns,
            )));
        }
        let client = builder
            .build()
            .map_err(|error| AutoDiscoveryError::RequestFailed(error.to_string()))?;
        Ok(Self { client })
    }
}

impl AutoDiscoveryRegistrar for ReqwestAutoDiscoveryRegistrar {
    fn register(
        &mut self,
        request: AutoDiscoveryRegisterRequest,
    ) -> Result<AutoDiscoveryCache, AutoDiscoveryError> {
        let payload = RegisterPayload {
            key: request.key.as_str(),
        };
        let mut http_request = self
            .client
            .post(&request.url)
            .header("Authorization", format!("Bearer {}", request.key))
            .header("Content-Type", "application/json")
            .json(&payload);
        for (name, value) in request.headers {
            http_request = http_request.header(name, value);
        }

        let response = http_request
            .send()
            .map_err(|error| AutoDiscoveryError::RequestFailed(error.to_string()))?;
        let status = response.status();
        let body = response
            .text()
            .map_err(|error| AutoDiscoveryError::RequestFailed(error.to_string()))?;
        if !status.is_success() {
            return Err(AutoDiscoveryError::RequestFailed(format!(
                "status={}: {}",
                status.as_u16(),
                body.trim()
            )));
        }

        let parsed: RegisterResponse = serde_json::from_str(&body).map_err(|error| {
            AutoDiscoveryError::RequestFailed(format!("invalid register response: {error}"))
        })?;
        if parsed.status != "success" {
            let message = non_empty_string(&parsed.message)
                .unwrap_or_else(|| "server returned non-success status".to_string());
            return Err(AutoDiscoveryError::RegisterRejected(message));
        }

        let data = parsed.data.ok_or_else(|| {
            AutoDiscoveryError::RequestFailed("register response missing data".to_string())
        })?;
        Ok(AutoDiscoveryCache {
            uuid: data.uuid,
            token: data.token,
        })
    }
}

#[derive(Serialize)]
struct RegisterPayload<'a> {
    key: &'a str,
}

#[derive(Deserialize)]
struct RegisterResponse {
    #[serde(default)]
    status: String,
    #[serde(default)]
    message: String,
    data: Option<RegisterData>,
}

#[derive(Deserialize)]
struct RegisterData {
    #[serde(default)]
    uuid: String,
    #[serde(default)]
    token: String,
}

fn read_cache(path: &Path) -> Option<AutoDiscoveryCache> {
    let contents = fs::read_to_string(path).ok()?;
    let cache = serde_json::from_str::<AutoDiscoveryCache>(&contents).ok()?;
    (!cache.token.trim().is_empty()).then_some(cache)
}

fn save_cache(path: &Path, cache: &AutoDiscoveryCache) -> Result<(), AutoDiscoveryError> {
    let contents = serde_json::to_string(cache)
        .map_err(|error| AutoDiscoveryError::CacheWrite(error.to_string()))?;
    fs::write(path, contents)
        .map_err(|error| AutoDiscoveryError::CacheWrite(format!("{}: {error}", path.display())))
}

fn read_hostname_file(path: &str) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .and_then(|value| non_empty_string(&value))
}

fn normalize_http_base(endpoint: &str) -> Result<String, AutoDiscoveryError> {
    let endpoint = endpoint.trim().trim_end_matches('/');
    if endpoint.is_empty() {
        return Err(AutoDiscoveryError::EmptyEndpoint);
    }

    if let Some(rest) = endpoint.strip_prefix("https://") {
        return Ok(format!("https://{}", endpoint_rest_with_ascii_host(rest)));
    }
    if let Some(rest) = endpoint.strip_prefix("http://") {
        return Ok(format!("http://{}", endpoint_rest_with_ascii_host(rest)));
    }

    let scheme = endpoint
        .split_once("://")
        .map(|(scheme, _)| scheme.to_string())
        .unwrap_or_else(|| "missing".to_string());
    Err(AutoDiscoveryError::UnsupportedScheme(scheme))
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

fn non_empty_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}
