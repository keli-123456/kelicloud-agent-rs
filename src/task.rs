use crate::config::AgentConfig;
use crate::linux_proc::CustomDnsResolver;
use crate::protocol::BackendMessage;
use crate::runtime::ControlMessageHandler;
use crate::transport::{access_headers, HeaderPair, TransportError};
use chrono::Utc;
use serde::Serialize;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskExecution {
    pub result: String,
    pub exit_code: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskResult {
    pub task_id: String,
    pub result: String,
    pub exit_code: i32,
    pub finished_at: String,
}

impl TaskResult {
    pub fn now(task_id: impl Into<String>, execution: TaskExecution) -> Self {
        Self {
            task_id: task_id.into(),
            result: execution.result,
            exit_code: execution.exit_code,
            finished_at: Utc::now().to_rfc3339(),
        }
    }

    fn fixed(task_id: impl Into<String>, result: impl Into<String>, exit_code: i32) -> Self {
        Self {
            task_id: task_id.into(),
            result: result.into(),
            exit_code,
            finished_at: Utc::now().to_rfc3339(),
        }
    }
}

pub trait TaskExecutor {
    fn execute(&self, command: &str) -> TaskExecution;
}

pub trait TaskResultUploader {
    fn upload_task_result(&self, result: &TaskResult) -> Result<(), TransportError>;
}

#[derive(Debug, Default, Clone)]
pub struct LinuxTaskExecutor;

impl TaskExecutor for LinuxTaskExecutor {
    fn execute(&self, command: &str) -> TaskExecution {
        let mut prepared = match prepare_task_command(command) {
            Ok(prepared) => prepared,
            Err(error) => {
                return TaskExecution {
                    result: format!("Failed to prepare command: {error}"),
                    exit_code: -1,
                }
            }
        };

        match prepared.command.output() {
            Ok(output) => TaskExecution {
                result: combined_output(&output.stdout, &output.stderr),
                exit_code: output.status.code().unwrap_or(-1),
            },
            Err(error) => TaskExecution {
                result: error.to_string(),
                exit_code: -1,
            },
        }
    }
}

#[derive(Debug)]
struct PreparedTaskCommand {
    command: Command,
    script_path: Option<PathBuf>,
}

impl Drop for PreparedTaskCommand {
    fn drop(&mut self) {
        if let Some(path) = self.script_path.as_ref() {
            let _ = fs::remove_file(path);
        }
    }
}

fn prepare_task_command(command: &str) -> Result<PreparedTaskCommand, std::io::Error> {
    if has_script_shebang(command) {
        let script_path = write_task_script(command)?;
        let mut task_command = Command::new(&script_path);
        task_command.stdout(Stdio::piped()).stderr(Stdio::piped());
        return Ok(PreparedTaskCommand {
            command: task_command,
            script_path: Some(script_path),
        });
    }

    let mut task_command = if let Some(bash_path) = find_in_path("bash") {
        let mut command_builder = Command::new(bash_path);
        command_builder.arg("-lc").arg(command);
        command_builder
    } else {
        let mut command_builder = Command::new("sh");
        command_builder.arg("-c").arg(command);
        command_builder
    };
    task_command.stdout(Stdio::piped()).stderr(Stdio::piped());

    Ok(PreparedTaskCommand {
        command: task_command,
        script_path: None,
    })
}

fn has_script_shebang(command: &str) -> bool {
    command.trim().starts_with("#!")
}

fn write_task_script(command: &str) -> Result<PathBuf, std::io::Error> {
    let path = unique_task_script_path();
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(command.as_bytes())?;
    file.flush()?;
    drop(file);
    set_executable(&path)?;
    Ok(path)
}

fn unique_task_script_path() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    env::temp_dir().join(format!("komari-task-{}-{now}.sh", std::process::id()))
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

fn find_in_path(binary: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|path| path.join(binary))
            .find(|path| path.is_file())
    })
}

fn combined_output(stdout: &[u8], stderr: &[u8]) -> String {
    let mut result = String::from_utf8_lossy(stdout).to_string();
    if !stderr.is_empty() {
        result.push('\n');
        result.push_str(&String::from_utf8_lossy(stderr));
    }
    result.replace("\r\n", "\n")
}

#[derive(Debug)]
pub struct TaskControlMessageHandler<E, U> {
    executor: E,
    uploader: U,
    remote_control_disabled: bool,
}

impl<E, U> TaskControlMessageHandler<E, U> {
    pub fn new(executor: E, uploader: U, remote_control_disabled: bool) -> Self {
        Self {
            executor,
            uploader,
            remote_control_disabled,
        }
    }
}

impl<E, U> ControlMessageHandler for TaskControlMessageHandler<E, U>
where
    E: TaskExecutor,
    U: TaskResultUploader,
{
    fn handle(&mut self, message: BackendMessage) {
        let BackendMessage::Exec { task_id, command } = message else {
            return;
        };
        if task_id.is_empty() {
            return;
        }

        let result = if command.trim().is_empty() {
            TaskResult::fixed(task_id, "No command provided", 0)
        } else if self.remote_control_disabled {
            TaskResult::fixed(task_id, "Remote control is disabled.", -1)
        } else {
            TaskResult::now(task_id, self.executor.execute(&command))
        };
        let _ = self.uploader.upload_task_result(&result);
    }
}

#[derive(Debug, Clone)]
pub struct HttpTaskResultUploader {
    client: reqwest::blocking::Client,
    url: String,
    headers: Vec<HeaderPair>,
    max_retries: u32,
    retry_delay: Duration,
}

impl HttpTaskResultUploader {
    pub fn from_config(config: &AgentConfig) -> Result<Self, TransportError> {
        Self::from_config_with_retry_delay(config, Duration::from_secs(2))
    }

    pub fn from_config_with_retry_delay(
        config: &AgentConfig,
        retry_delay: Duration,
    ) -> Result<Self, TransportError> {
        let mut builder =
            reqwest::blocking::Client::builder().danger_accept_invalid_certs(config.insecure);
        let custom_dns = config.custom_dns.trim();
        if !custom_dns.is_empty() {
            builder = builder.dns_resolver(Arc::new(CustomDnsResolver::new(custom_dns)));
        }
        let client = builder
            .build()
            .map_err(|error| TransportError::RequestFailed(error.to_string()))?;

        Ok(Self {
            client,
            url: build_task_result_url(&config.endpoint, &config.token)?,
            headers: access_headers(config),
            max_retries: config.max_retries,
            retry_delay,
        })
    }
}

impl TaskResultUploader for HttpTaskResultUploader {
    fn upload_task_result(&self, result: &TaskResult) -> Result<(), TransportError> {
        let total_attempts = (self.max_retries + 1).max(1);
        let mut last_error = None;

        for attempt in 1..=total_attempts {
            let mut request = self.client.post(&self.url).json(result);
            for (name, value) in &self.headers {
                request = request.header(name, value);
            }

            match request.send() {
                Ok(response) if response.status().is_success() => return Ok(()),
                Ok(response) => {
                    let status = response.status();
                    let body = response.text().unwrap_or_default();
                    last_error = Some(TransportError::RequestFailed(format!(
                        "status={status} {body}"
                    )));
                }
                Err(error) => {
                    last_error = Some(TransportError::RequestFailed(error.to_string()));
                }
            }

            if attempt < total_attempts && !self.retry_delay.is_zero() {
                std::thread::sleep(self.retry_delay);
            }
        }

        Err(last_error.unwrap_or_else(|| {
            TransportError::RequestFailed("task result upload failed".to_string())
        }))
    }
}

pub fn build_task_result_url(endpoint: &str, token: &str) -> Result<String, TransportError> {
    let endpoint = normalize_http_base(endpoint)?;
    let token = require_non_empty(token, TransportError::EmptyToken)?;
    Ok(format!(
        "{endpoint}/api/clients/task/result?token={}",
        percent_encode(token)
    ))
}

fn normalize_http_base(endpoint: &str) -> Result<String, TransportError> {
    let endpoint = endpoint.trim().trim_end_matches('/');
    if endpoint.is_empty() {
        return Err(TransportError::EmptyEndpoint);
    }

    if endpoint.starts_with("https://") || endpoint.starts_with("http://") {
        return Ok(endpoint.to_string());
    }

    let scheme = endpoint
        .split_once("://")
        .map(|(scheme, _)| scheme.to_string())
        .unwrap_or_else(|| "missing".to_string());
    Err(TransportError::UnsupportedScheme(scheme))
}

fn require_non_empty(value: &str, error: TransportError) -> Result<&str, TransportError> {
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
