use serde::Deserialize;
use std::env;
use std::error::Error;
use std::fmt;
use std::fs;

#[derive(Debug, Clone, PartialEq)]
pub struct AgentConfig {
    pub endpoint: String,
    pub token: String,
    pub auto_discovery_key: String,
    pub insecure: bool,
    pub disable_web_ssh: bool,
    pub tunnel_control_enabled: bool,
    pub tunnel_data_enabled: bool,
    pub interval_seconds: f64,
    pub max_retries: u32,
    pub reconnect_interval_seconds: u64,
    pub info_report_interval_minutes: u64,
    pub cf_access_client_id: String,
    pub cf_access_client_secret: String,
    pub include_nics: String,
    pub exclude_nics: String,
    pub include_mountpoints: String,
    pub custom_ipv4: String,
    pub custom_ipv6: String,
    pub custom_dns: String,
    pub get_ip_addr_from_nic: bool,
    pub memory_include_cache: bool,
    pub memory_report_raw_used: bool,
    pub enable_gpu: bool,
    pub month_rotate: u32,
    pub host_proc: String,
    pub once: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    MissingEndpoint,
    MissingToken,
    MissingValue(&'static str),
    InvalidValue(&'static str, String),
    ConfigFileRead(String),
    ConfigFileParse(String),
    UnknownArgument(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEndpoint => write!(f, "endpoint is required"),
            Self::MissingToken => write!(f, "token is required"),
            Self::MissingValue(flag) => write!(f, "{flag} requires a value"),
            Self::InvalidValue(flag, value) => write!(f, "{flag} has invalid value: {value}"),
            Self::ConfigFileRead(message) => write!(f, "failed to read config file: {message}"),
            Self::ConfigFileParse(message) => write!(f, "failed to parse config file: {message}"),
            Self::UnknownArgument(arg) => write!(f, "unknown argument: {arg}"),
        }
    }
}

impl Error for ConfigError {}

impl AgentConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_args_and_env(env::args(), |key| env::var(key).ok())
    }

    pub fn from_args_and_env<I, S, F>(args: I, env_lookup: F) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
        F: Fn(&str) -> Option<String>,
    {
        let mut endpoint = clean_optional(env_lookup("AGENT_ENDPOINT"));
        let mut token = clean_optional(env_lookup("AGENT_TOKEN"));
        let mut auto_discovery_key = clean_optional(env_lookup("AGENT_AUTO_DISCOVERY_KEY"));
        let mut insecure = env_lookup("AGENT_IGNORE_UNSAFE_CERT")
            .or_else(|| env_lookup("AGENT_INSECURE"))
            .as_deref()
            .map(parse_bool)
            .unwrap_or(false);
        let mut disable_web_ssh = env_lookup("AGENT_DISABLE_WEB_SSH")
            .as_deref()
            .map(parse_bool)
            .unwrap_or(false);
        let mut tunnel_control_enabled = env_lookup("AGENT_TUNNEL_CONTROL_ENABLED")
            .as_deref()
            .and_then(parse_tunnel_control_enabled)
            .unwrap_or(true);
        let mut tunnel_data_enabled = false;
        let mut interval_seconds = parse_env_f64(&env_lookup, "AGENT_INTERVAL", 1.0)?;
        let mut max_retries = parse_env_u32(&env_lookup, "AGENT_MAX_RETRIES", 3)?;
        let mut reconnect_interval_seconds =
            parse_env_u64(&env_lookup, "AGENT_RECONNECT_INTERVAL", 5)?;
        let mut info_report_interval_minutes =
            parse_env_u64_allow_zero(&env_lookup, "AGENT_INFO_REPORT_INTERVAL", 5)?;
        let mut cf_access_client_id =
            clean_optional(env_lookup("AGENT_CF_ACCESS_CLIENT_ID")).unwrap_or_default();
        let mut cf_access_client_secret =
            clean_optional(env_lookup("AGENT_CF_ACCESS_CLIENT_SECRET")).unwrap_or_default();
        let mut include_nics = clean_optional(env_lookup("AGENT_INCLUDE_NICS")).unwrap_or_default();
        let mut exclude_nics = clean_optional(env_lookup("AGENT_EXCLUDE_NICS")).unwrap_or_default();
        let mut include_mountpoints =
            clean_optional(env_lookup("AGENT_INCLUDE_MOUNTPOINTS")).unwrap_or_default();
        let mut custom_ipv4 = clean_optional(env_lookup("AGENT_CUSTOM_IPV4")).unwrap_or_default();
        let mut custom_ipv6 = clean_optional(env_lookup("AGENT_CUSTOM_IPV6")).unwrap_or_default();
        let mut custom_dns = clean_optional(env_lookup("AGENT_CUSTOM_DNS")).unwrap_or_default();
        let mut get_ip_addr_from_nic = env_lookup("AGENT_GET_IP_ADDR_FROM_NIC")
            .as_deref()
            .map(parse_bool)
            .unwrap_or(false);
        let mut memory_include_cache = env_lookup("AGENT_MEMORY_INCLUDE_CACHE")
            .as_deref()
            .map(parse_bool)
            .unwrap_or(false);
        let mut memory_report_raw_used = env_lookup("AGENT_MEMORY_REPORT_RAW_USED")
            .as_deref()
            .map(parse_bool)
            .unwrap_or(false);
        let mut enable_gpu = env_lookup("AGENT_ENABLE_GPU")
            .as_deref()
            .map(parse_bool)
            .unwrap_or(false);
        let mut month_rotate = parse_env_u32(&env_lookup, "AGENT_MONTH_ROTATE", 0)?;
        let mut host_proc = clean_optional(env_lookup("HOST_PROC")).unwrap_or_default();
        let mut config_file = clean_optional(env_lookup("AGENT_CONFIG_FILE")).unwrap_or_default();
        let mut once = env_lookup("AGENT_ONCE")
            .as_deref()
            .map(parse_bool)
            .unwrap_or(false);

        let mut iter = args.into_iter();
        let _program = iter.next();
        while let Some(arg) = iter.next() {
            let arg = arg.as_ref();
            match arg {
                "--endpoint" => {
                    endpoint = Some(next_value(&mut iter, "--endpoint")?);
                }
                "-e" => {
                    endpoint = Some(next_value(&mut iter, "-e")?);
                }
                "--token" => {
                    token = Some(next_value(&mut iter, "--token")?);
                }
                "-t" => {
                    token = Some(next_value(&mut iter, "-t")?);
                }
                "--auto-discovery" => {
                    auto_discovery_key = Some(next_value(&mut iter, "--auto-discovery")?);
                }
                "--insecure" => {
                    insecure = true;
                }
                "--ignore-unsafe-cert" => {
                    insecure = true;
                }
                "-u" => {
                    insecure = true;
                }
                "--disable-web-ssh" => {
                    disable_web_ssh = true;
                }
                "--once" => {
                    once = true;
                }
                "--interval" => {
                    interval_seconds =
                        parse_f64("--interval", &next_value(&mut iter, "--interval")?)?;
                }
                "-i" => {
                    interval_seconds = parse_f64("-i", &next_value(&mut iter, "-i")?)?;
                }
                "--max-retries" => {
                    max_retries =
                        parse_u32("--max-retries", &next_value(&mut iter, "--max-retries")?)?;
                }
                "-r" => {
                    max_retries = parse_u32("-r", &next_value(&mut iter, "-r")?)?;
                }
                "--reconnect-interval" => {
                    reconnect_interval_seconds = parse_u64(
                        "--reconnect-interval",
                        &next_value(&mut iter, "--reconnect-interval")?,
                    )?;
                }
                "-c" => {
                    reconnect_interval_seconds = parse_u64("-c", &next_value(&mut iter, "-c")?)?;
                }
                "--info-report-interval" => {
                    info_report_interval_minutes = parse_u64_allow_zero(
                        "--info-report-interval",
                        &next_value(&mut iter, "--info-report-interval")?,
                    )?;
                }
                "--cf-access-client-id" => {
                    cf_access_client_id = next_value(&mut iter, "--cf-access-client-id")?;
                }
                "--cf-access-client-secret" => {
                    cf_access_client_secret = next_value(&mut iter, "--cf-access-client-secret")?;
                }
                "--include-nics" => {
                    include_nics = next_value(&mut iter, "--include-nics")?;
                }
                "--exclude-nics" => {
                    exclude_nics = next_value(&mut iter, "--exclude-nics")?;
                }
                "--include-mountpoints" | "--include-mountpoint" => {
                    include_mountpoints = next_value(&mut iter, "--include-mountpoint")?;
                }
                "--custom-ipv4" => {
                    custom_ipv4 = next_value(&mut iter, "--custom-ipv4")?;
                }
                "--custom-ipv6" => {
                    custom_ipv6 = next_value(&mut iter, "--custom-ipv6")?;
                }
                "--custom-dns" => {
                    custom_dns = next_value(&mut iter, "--custom-dns")?;
                }
                "--get-ip-addr-from-nic" => {
                    get_ip_addr_from_nic = true;
                }
                "--memory-include-cache" => {
                    memory_include_cache = true;
                }
                "--memory-exclude-bcf" => {
                    memory_report_raw_used = true;
                }
                "--memory-mode-available" | "-memory-mode-available" => {}
                "--enable-gpu" | "--gpu" => {
                    enable_gpu = true;
                }
                "--month-rotate" => {
                    month_rotate =
                        parse_u32("--month-rotate", &next_value(&mut iter, "--month-rotate")?)?;
                }
                "--host-proc" => {
                    host_proc = next_value(&mut iter, "--host-proc")?;
                }
                "--config" => {
                    config_file = next_value(&mut iter, "--config")?;
                }
                _ if arg.starts_with("--endpoint=") => {
                    endpoint = clean_required(&arg["--endpoint=".len()..], "--endpoint")?;
                }
                _ if arg.starts_with("-e=") => {
                    endpoint = clean_required(&arg["-e=".len()..], "-e")?;
                }
                _ if arg.starts_with("--token=") => {
                    token = clean_required(&arg["--token=".len()..], "--token")?;
                }
                _ if arg.starts_with("-t=") => {
                    token = clean_required(&arg["-t=".len()..], "-t")?;
                }
                _ if arg.starts_with("--auto-discovery=") => {
                    auto_discovery_key =
                        clean_required(&arg["--auto-discovery=".len()..], "--auto-discovery")?;
                }
                _ if arg.starts_with("--interval=") => {
                    interval_seconds = parse_f64("--interval", &arg["--interval=".len()..])?;
                }
                _ if arg.starts_with("-i=") => {
                    interval_seconds = parse_f64("-i", &arg["-i=".len()..])?;
                }
                _ if arg.starts_with("--max-retries=") => {
                    max_retries = parse_u32("--max-retries", &arg["--max-retries=".len()..])?;
                }
                _ if arg.starts_with("-r=") => {
                    max_retries = parse_u32("-r", &arg["-r=".len()..])?;
                }
                _ if arg.starts_with("--reconnect-interval=") => {
                    reconnect_interval_seconds = parse_u64(
                        "--reconnect-interval",
                        &arg["--reconnect-interval=".len()..],
                    )?;
                }
                _ if arg.starts_with("-c=") => {
                    reconnect_interval_seconds = parse_u64("-c", &arg["-c=".len()..])?;
                }
                _ if arg.starts_with("--info-report-interval=") => {
                    info_report_interval_minutes = parse_u64_allow_zero(
                        "--info-report-interval",
                        &arg["--info-report-interval=".len()..],
                    )?;
                }
                _ if arg.starts_with("--cf-access-client-id=") => {
                    cf_access_client_id = clean_required(
                        &arg["--cf-access-client-id=".len()..],
                        "--cf-access-client-id",
                    )?
                    .unwrap();
                }
                _ if arg.starts_with("--cf-access-client-secret=") => {
                    cf_access_client_secret = clean_required(
                        &arg["--cf-access-client-secret=".len()..],
                        "--cf-access-client-secret",
                    )?
                    .unwrap();
                }
                _ if arg.starts_with("--include-nics=") => {
                    include_nics =
                        clean_required(&arg["--include-nics=".len()..], "--include-nics")?.unwrap();
                }
                _ if arg.starts_with("--exclude-nics=") => {
                    exclude_nics =
                        clean_required(&arg["--exclude-nics=".len()..], "--exclude-nics")?.unwrap();
                }
                _ if arg.starts_with("--include-mountpoints=") => {
                    include_mountpoints = clean_required(
                        &arg["--include-mountpoints=".len()..],
                        "--include-mountpoints",
                    )?
                    .unwrap();
                }
                _ if arg.starts_with("--include-mountpoint=") => {
                    include_mountpoints = clean_required(
                        &arg["--include-mountpoint=".len()..],
                        "--include-mountpoint",
                    )?
                    .unwrap();
                }
                _ if arg.starts_with("--custom-ipv4=") => {
                    custom_ipv4 =
                        clean_required(&arg["--custom-ipv4=".len()..], "--custom-ipv4")?.unwrap();
                }
                _ if arg.starts_with("--custom-ipv6=") => {
                    custom_ipv6 =
                        clean_required(&arg["--custom-ipv6=".len()..], "--custom-ipv6")?.unwrap();
                }
                _ if arg.starts_with("--custom-dns=") => {
                    custom_dns =
                        clean_required(&arg["--custom-dns=".len()..], "--custom-dns")?.unwrap();
                }
                _ if arg.starts_with("--month-rotate=") => {
                    month_rotate = parse_u32("--month-rotate", &arg["--month-rotate=".len()..])?;
                }
                _ if arg.starts_with("--host-proc=") => {
                    host_proc =
                        clean_required(&arg["--host-proc=".len()..], "--host-proc")?.unwrap();
                }
                _ if arg.starts_with("--config=") => {
                    config_file = clean_required(&arg["--config=".len()..], "--config")?.unwrap();
                }
                _ => {}
            }
        }

        apply_optional_string_env(&env_lookup, "AGENT_ENDPOINT", &mut endpoint);
        apply_optional_string_env(&env_lookup, "AGENT_TOKEN", &mut token);
        apply_optional_string_env(
            &env_lookup,
            "AGENT_AUTO_DISCOVERY_KEY",
            &mut auto_discovery_key,
        );
        apply_bool_true_env(&env_lookup, "AGENT_IGNORE_UNSAFE_CERT", &mut insecure);
        apply_bool_true_env(&env_lookup, "AGENT_INSECURE", &mut insecure);
        apply_bool_true_env(&env_lookup, "AGENT_DISABLE_WEB_SSH", &mut disable_web_ssh);
        apply_tunnel_control_enabled_env(&env_lookup, &mut tunnel_control_enabled);
        apply_bool_env(
            &env_lookup,
            "AGENT_TUNNEL_DATA_ENABLED",
            &mut tunnel_data_enabled,
        );
        apply_f64_env(&env_lookup, "AGENT_INTERVAL", &mut interval_seconds);
        apply_u32_env(&env_lookup, "AGENT_MAX_RETRIES", &mut max_retries);
        apply_u64_env(
            &env_lookup,
            "AGENT_RECONNECT_INTERVAL",
            &mut reconnect_interval_seconds,
        );
        apply_u64_env_allow_zero(
            &env_lookup,
            "AGENT_INFO_REPORT_INTERVAL",
            &mut info_report_interval_minutes,
        );
        apply_string_env(
            &env_lookup,
            "AGENT_CF_ACCESS_CLIENT_ID",
            &mut cf_access_client_id,
        );
        apply_string_env(
            &env_lookup,
            "AGENT_CF_ACCESS_CLIENT_SECRET",
            &mut cf_access_client_secret,
        );
        apply_string_env(&env_lookup, "AGENT_INCLUDE_NICS", &mut include_nics);
        apply_string_env(&env_lookup, "AGENT_EXCLUDE_NICS", &mut exclude_nics);
        apply_string_env(
            &env_lookup,
            "AGENT_INCLUDE_MOUNTPOINTS",
            &mut include_mountpoints,
        );
        apply_string_env(&env_lookup, "AGENT_CUSTOM_IPV4", &mut custom_ipv4);
        apply_string_env(&env_lookup, "AGENT_CUSTOM_IPV6", &mut custom_ipv6);
        apply_string_env(&env_lookup, "AGENT_CUSTOM_DNS", &mut custom_dns);
        apply_bool_true_env(
            &env_lookup,
            "AGENT_GET_IP_ADDR_FROM_NIC",
            &mut get_ip_addr_from_nic,
        );
        apply_bool_true_env(
            &env_lookup,
            "AGENT_MEMORY_INCLUDE_CACHE",
            &mut memory_include_cache,
        );
        apply_bool_true_env(
            &env_lookup,
            "AGENT_MEMORY_REPORT_RAW_USED",
            &mut memory_report_raw_used,
        );
        apply_bool_true_env(&env_lookup, "AGENT_ENABLE_GPU", &mut enable_gpu);
        apply_u32_env(&env_lookup, "AGENT_MONTH_ROTATE", &mut month_rotate);
        apply_string_env(&env_lookup, "HOST_PROC", &mut host_proc);
        apply_string_env(&env_lookup, "AGENT_CONFIG_FILE", &mut config_file);
        apply_bool_true_env(&env_lookup, "AGENT_ONCE", &mut once);

        if !config_file.is_empty() {
            let file_config = read_file_config(&config_file)?;
            if let Some(value) = file_config.endpoint {
                endpoint = clean_config_required_string(value);
            }
            if let Some(value) = file_config.token {
                token = clean_config_required_string(value);
            }
            if let Some(value) = file_config.auto_discovery_key {
                auto_discovery_key = clean_config_required_string(value);
            }
            if let Some(value) = file_config.ignore_unsafe_cert {
                insecure = value;
            }
            if let Some(value) = file_config.disable_web_ssh {
                disable_web_ssh = value;
            }
            if let Some(value) = file_config.tunnel_control_enabled {
                tunnel_control_enabled = value;
            }
            if let Some(value) = file_config.tunnel_data_enabled {
                tunnel_data_enabled = value;
            }
            if let Some(value) = file_config.interval {
                interval_seconds = validate_positive_f64("interval", value)?;
            }
            if let Some(value) = file_config.max_retries {
                max_retries = value;
            }
            if let Some(value) = file_config.reconnect_interval {
                reconnect_interval_seconds = validate_positive_u64("reconnect_interval", value)?;
            }
            if let Some(value) = file_config.info_report_interval {
                info_report_interval_minutes = value;
            }
            if let Some(value) = file_config.cf_access_client_id {
                cf_access_client_id = clean_config_string(value);
            }
            if let Some(value) = file_config.cf_access_client_secret {
                cf_access_client_secret = clean_config_string(value);
            }
            if let Some(value) = file_config.include_nics {
                include_nics = clean_config_string(value);
            }
            if let Some(value) = file_config.exclude_nics {
                exclude_nics = clean_config_string(value);
            }
            if let Some(value) = file_config.include_mountpoints {
                include_mountpoints = clean_config_string(value);
            }
            if let Some(value) = file_config.custom_ipv4 {
                custom_ipv4 = clean_config_string(value);
            }
            if let Some(value) = file_config.custom_ipv6 {
                custom_ipv6 = clean_config_string(value);
            }
            if let Some(value) = file_config.custom_dns {
                custom_dns = clean_config_string(value);
            }
            if let Some(value) = file_config.get_ip_addr_from_nic {
                get_ip_addr_from_nic = value;
            }
            if let Some(value) = file_config.memory_include_cache {
                memory_include_cache = value;
            }
            if let Some(value) = file_config.memory_report_raw_used {
                memory_report_raw_used = value;
            }
            if let Some(value) = file_config.enable_gpu {
                enable_gpu = value;
            }
            if let Some(value) = file_config.month_rotate {
                month_rotate = value;
            }
            if let Some(value) = file_config.host_proc {
                host_proc = clean_config_string(value);
            }
        }

        let endpoint = endpoint.ok_or(ConfigError::MissingEndpoint)?;
        let token = token.unwrap_or_default();
        let auto_discovery_key = auto_discovery_key.unwrap_or_default();
        if token.is_empty() && auto_discovery_key.is_empty() {
            return Err(ConfigError::MissingToken);
        }

        Ok(Self {
            endpoint,
            token,
            auto_discovery_key,
            insecure,
            disable_web_ssh,
            tunnel_control_enabled,
            tunnel_data_enabled,
            interval_seconds,
            max_retries,
            reconnect_interval_seconds,
            info_report_interval_minutes,
            cf_access_client_id,
            cf_access_client_secret,
            include_nics,
            exclude_nics,
            include_mountpoints,
            custom_ipv4,
            custom_ipv6,
            custom_dns,
            get_ip_addr_from_nic,
            memory_include_cache,
            memory_report_raw_used,
            enable_gpu,
            month_rotate,
            host_proc,
            once,
        })
    }
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    endpoint: Option<String>,
    token: Option<String>,
    auto_discovery_key: Option<String>,
    ignore_unsafe_cert: Option<bool>,
    disable_web_ssh: Option<bool>,
    tunnel_control_enabled: Option<bool>,
    tunnel_data_enabled: Option<bool>,
    interval: Option<f64>,
    max_retries: Option<u32>,
    reconnect_interval: Option<u64>,
    info_report_interval: Option<u64>,
    cf_access_client_id: Option<String>,
    cf_access_client_secret: Option<String>,
    include_nics: Option<String>,
    exclude_nics: Option<String>,
    include_mountpoints: Option<String>,
    custom_ipv4: Option<String>,
    custom_ipv6: Option<String>,
    custom_dns: Option<String>,
    get_ip_addr_from_nic: Option<bool>,
    memory_include_cache: Option<bool>,
    memory_report_raw_used: Option<bool>,
    enable_gpu: Option<bool>,
    month_rotate: Option<u32>,
    host_proc: Option<String>,
}

fn read_file_config(path: &str) -> Result<FileConfig, ConfigError> {
    let contents = fs::read_to_string(path)
        .map_err(|error| ConfigError::ConfigFileRead(format!("{path}: {error}")))?;
    serde_json::from_str(&contents)
        .map_err(|error| ConfigError::ConfigFileParse(format!("{path}: {error}")))
}

fn next_value<I, S>(iter: &mut I, flag: &'static str) -> Result<String, ConfigError>
where
    I: Iterator<Item = S>,
    S: AsRef<str>,
{
    let value = iter.next().ok_or(ConfigError::MissingValue(flag))?;
    clean_required(value.as_ref(), flag)?.ok_or(ConfigError::MissingValue(flag))
}

fn clean_required(value: &str, flag: &'static str) -> Result<Option<String>, ConfigError> {
    let value = value.trim();
    if value.is_empty() {
        Err(ConfigError::MissingValue(flag))
    } else {
        Ok(Some(value.to_string()))
    }
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    })
}

fn clean_config_required_string(value: String) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn clean_config_string(value: String) -> String {
    value.trim().to_string()
}

fn parse_bool(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true")
}

fn parse_tunnel_control_enabled(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "enabled" | "enable" | "auto" | "on" | "yes" => Some(true),
        "0" | "false" | "disabled" | "disable" | "off" | "no" => Some(false),
        _ => None,
    }
}

fn parse_env_f64<F>(env_lookup: &F, key: &'static str, default: f64) -> Result<f64, ConfigError>
where
    F: Fn(&str) -> Option<String>,
{
    match clean_optional(env_lookup(key)) {
        Some(value) => Ok(parse_f64(key, &value).unwrap_or(default)),
        None => Ok(default),
    }
}

fn parse_env_u32<F>(env_lookup: &F, key: &'static str, default: u32) -> Result<u32, ConfigError>
where
    F: Fn(&str) -> Option<String>,
{
    match clean_optional(env_lookup(key)) {
        Some(value) => Ok(parse_u32(key, &value).unwrap_or(default)),
        None => Ok(default),
    }
}

fn parse_env_u64<F>(env_lookup: &F, key: &'static str, default: u64) -> Result<u64, ConfigError>
where
    F: Fn(&str) -> Option<String>,
{
    match clean_optional(env_lookup(key)) {
        Some(value) => Ok(parse_u64(key, &value).unwrap_or(default)),
        None => Ok(default),
    }
}

fn parse_env_u64_allow_zero<F>(
    env_lookup: &F,
    key: &'static str,
    default: u64,
) -> Result<u64, ConfigError>
where
    F: Fn(&str) -> Option<String>,
{
    match clean_optional(env_lookup(key)) {
        Some(value) => Ok(parse_u64_allow_zero(key, &value).unwrap_or(default)),
        None => Ok(default),
    }
}

fn apply_optional_string_env<F>(env_lookup: &F, key: &str, target: &mut Option<String>)
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(value) = clean_optional(env_lookup(key)) {
        *target = Some(value);
    }
}

fn apply_string_env<F>(env_lookup: &F, key: &str, target: &mut String)
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(value) = clean_optional(env_lookup(key)) {
        *target = value;
    }
}

fn apply_bool_true_env<F>(env_lookup: &F, key: &str, target: &mut bool)
where
    F: Fn(&str) -> Option<String>,
{
    if env_lookup(key).as_deref().is_some_and(parse_bool) {
        *target = true;
    }
}

fn apply_bool_env<F>(env_lookup: &F, key: &str, target: &mut bool)
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(value) = env_lookup(key) {
        *target = parse_bool(&value);
    }
}

fn apply_tunnel_control_enabled_env<F>(env_lookup: &F, target: &mut bool)
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(value) = env_lookup("AGENT_TUNNEL_CONTROL_ENABLED")
        .as_deref()
        .and_then(parse_tunnel_control_enabled)
    {
        *target = value;
    }
}

fn apply_f64_env<F>(env_lookup: &F, key: &'static str, target: &mut f64)
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(value) = clean_optional(env_lookup(key)) {
        if let Ok(parsed) = parse_f64(key, &value) {
            *target = parsed;
        }
    }
}

fn apply_u32_env<F>(env_lookup: &F, key: &'static str, target: &mut u32)
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(value) = clean_optional(env_lookup(key)) {
        if let Ok(parsed) = parse_u32(key, &value) {
            *target = parsed;
        }
    }
}

fn apply_u64_env<F>(env_lookup: &F, key: &'static str, target: &mut u64)
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(value) = clean_optional(env_lookup(key)) {
        if let Ok(parsed) = parse_u64(key, &value) {
            *target = parsed;
        }
    }
}

fn apply_u64_env_allow_zero<F>(env_lookup: &F, key: &'static str, target: &mut u64)
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(value) = clean_optional(env_lookup(key)) {
        if let Ok(parsed) = parse_u64_allow_zero(key, &value) {
            *target = parsed;
        }
    }
}

fn parse_f64(flag: &'static str, value: &str) -> Result<f64, ConfigError> {
    let parsed = value
        .trim()
        .parse::<f64>()
        .map_err(|_| ConfigError::InvalidValue(flag, value.to_string()))?;
    validate_positive_f64(flag, parsed)
}

fn validate_positive_f64(flag: &'static str, parsed: f64) -> Result<f64, ConfigError> {
    if !parsed.is_finite() || parsed <= 0.0 {
        return Err(ConfigError::InvalidValue(flag, parsed.to_string()));
    }
    Ok(parsed)
}

fn parse_u32(flag: &'static str, value: &str) -> Result<u32, ConfigError> {
    value
        .trim()
        .parse::<u32>()
        .map_err(|_| ConfigError::InvalidValue(flag, value.to_string()))
}

fn parse_u64(flag: &'static str, value: &str) -> Result<u64, ConfigError> {
    let parsed = value
        .trim()
        .parse::<u64>()
        .map_err(|_| ConfigError::InvalidValue(flag, value.to_string()))?;
    validate_positive_u64(flag, parsed)
}

fn parse_u64_allow_zero(flag: &'static str, value: &str) -> Result<u64, ConfigError> {
    value
        .trim()
        .parse::<u64>()
        .map_err(|_| ConfigError::InvalidValue(flag, value.to_string()))
}

fn validate_positive_u64(flag: &'static str, parsed: u64) -> Result<u64, ConfigError> {
    if parsed == 0 {
        return Err(ConfigError::InvalidValue(flag, parsed.to_string()));
    }
    Ok(parsed)
}
