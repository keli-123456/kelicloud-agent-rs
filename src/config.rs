use std::env;
use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct AgentConfig {
    pub endpoint: String,
    pub token: String,
    pub insecure: bool,
    pub disable_web_ssh: bool,
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
    UnknownArgument(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEndpoint => write!(f, "endpoint is required"),
            Self::MissingToken => write!(f, "token is required"),
            Self::MissingValue(flag) => write!(f, "{flag} requires a value"),
            Self::InvalidValue(flag, value) => write!(f, "{flag} has invalid value: {value}"),
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
        let mut insecure = env_lookup("AGENT_IGNORE_UNSAFE_CERT")
            .or_else(|| env_lookup("AGENT_INSECURE"))
            .as_deref()
            .map(parse_bool)
            .unwrap_or(false);
        let mut disable_web_ssh = env_lookup("AGENT_DISABLE_WEB_SSH")
            .as_deref()
            .map(parse_bool)
            .unwrap_or(false);
        let mut interval_seconds = parse_env_f64(&env_lookup, "AGENT_INTERVAL", 1.0)?;
        let mut max_retries = parse_env_u32(&env_lookup, "AGENT_MAX_RETRIES", 3)?;
        let mut reconnect_interval_seconds =
            parse_env_u64(&env_lookup, "AGENT_RECONNECT_INTERVAL", 5)?;
        let mut info_report_interval_minutes =
            parse_env_u64(&env_lookup, "AGENT_INFO_REPORT_INTERVAL", 5)?;
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
                    info_report_interval_minutes = parse_u64(
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
                "--get-ip-addr-from-nic" => {
                    get_ip_addr_from_nic = true;
                }
                "--memory-include-cache" => {
                    memory_include_cache = true;
                }
                "--memory-exclude-bcf" => {
                    memory_report_raw_used = true;
                }
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
                    info_report_interval_minutes = parse_u64(
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
                _ if arg.starts_with("--month-rotate=") => {
                    month_rotate = parse_u32("--month-rotate", &arg["--month-rotate=".len()..])?;
                }
                _ if arg.starts_with("--host-proc=") => {
                    host_proc =
                        clean_required(&arg["--host-proc=".len()..], "--host-proc")?.unwrap();
                }
                _ => return Err(ConfigError::UnknownArgument(arg.to_string())),
            }
        }

        Ok(Self {
            endpoint: endpoint.ok_or(ConfigError::MissingEndpoint)?,
            token: token.ok_or(ConfigError::MissingToken)?,
            insecure,
            disable_web_ssh,
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

fn parse_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "y" | "on"
    )
}

fn parse_env_f64<F>(env_lookup: &F, key: &'static str, default: f64) -> Result<f64, ConfigError>
where
    F: Fn(&str) -> Option<String>,
{
    match clean_optional(env_lookup(key)) {
        Some(value) => parse_f64(key, &value),
        None => Ok(default),
    }
}

fn parse_env_u32<F>(env_lookup: &F, key: &'static str, default: u32) -> Result<u32, ConfigError>
where
    F: Fn(&str) -> Option<String>,
{
    match clean_optional(env_lookup(key)) {
        Some(value) => parse_u32(key, &value),
        None => Ok(default),
    }
}

fn parse_env_u64<F>(env_lookup: &F, key: &'static str, default: u64) -> Result<u64, ConfigError>
where
    F: Fn(&str) -> Option<String>,
{
    match clean_optional(env_lookup(key)) {
        Some(value) => parse_u64(key, &value),
        None => Ok(default),
    }
}

fn parse_f64(flag: &'static str, value: &str) -> Result<f64, ConfigError> {
    let parsed = value
        .trim()
        .parse::<f64>()
        .map_err(|_| ConfigError::InvalidValue(flag, value.to_string()))?;
    if parsed <= 0.0 {
        return Err(ConfigError::InvalidValue(flag, value.to_string()));
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
    if parsed == 0 {
        return Err(ConfigError::InvalidValue(flag, value.to_string()));
    }
    Ok(parsed)
}
