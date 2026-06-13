use crate::config::AgentConfig;
use crate::protocol::{build_terminal_ws_url, BackendMessage, ProtocolError};
use crate::runtime::ControlMessageHandler;
use crate::smoke_summary::smoke_event_line;
use crate::token::SharedAgentToken;
use crate::transport::{access_headers, connect_websocket_request, HeaderPair, TransportError};
use serde::Deserialize;
use std::error::Error;
use std::fmt;
use std::net::TcpStream;
use std::sync::Arc;
use std::thread;
use tungstenite::client::IntoClientRequest;
use tungstenite::http::{HeaderName, HeaderValue};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

#[cfg(target_os = "linux")]
use std::io::ErrorKind;
#[cfg(target_os = "linux")]
use std::time::Duration;

const WEB_SSH_DISABLED_MESSAGE: &str =
    "\n\nWeb SSH is disabled. Enable it by running without the --disable-web-ssh flag.";

#[derive(Debug)]
pub enum TerminalError {
    Protocol(ProtocolError),
    Transport(TransportError),
    Io(std::io::Error),
    UnsupportedPlatform,
}

impl fmt::Display for TerminalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(error) => write!(f, "{error}"),
            Self::Transport(error) => write!(f, "{error}"),
            Self::Io(error) => write!(f, "{error}"),
            Self::UnsupportedPlatform => write!(f, "terminal is supported on Linux only"),
        }
    }
}

impl Error for TerminalError {}

impl From<ProtocolError> for TerminalError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}

impl From<TransportError> for TerminalError {
    fn from(value: TransportError) -> Self {
        Self::Transport(value)
    }
}

impl From<std::io::Error> for TerminalError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub trait TerminalConnector: Send + Sync + 'static {
    fn start_terminal(
        &self,
        request_id: &str,
        remote_control_disabled: bool,
    ) -> Result<(), TerminalError>;
}

#[derive(Debug)]
pub struct TerminalControlMessageHandler<C> {
    connector: Arc<C>,
    remote_control_disabled: bool,
}

impl<C> TerminalControlMessageHandler<C> {
    pub fn new(connector: C, remote_control_disabled: bool) -> Self {
        Self {
            connector: Arc::new(connector),
            remote_control_disabled,
        }
    }
}

impl<C> ControlMessageHandler for TerminalControlMessageHandler<C>
where
    C: TerminalConnector,
{
    fn handle(&mut self, message: BackendMessage) {
        let BackendMessage::Terminal { request_id } = message else {
            return;
        };
        if request_id.is_empty() {
            return;
        }

        let connector = Arc::clone(&self.connector);
        let remote_control_disabled = self.remote_control_disabled;
        thread::spawn(move || {
            let _ = connector.start_terminal(&request_id, remote_control_disabled);
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalClientCommand {
    Input(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    Ignore,
}

#[derive(Debug, Deserialize)]
struct RawTerminalClientCommand {
    #[serde(rename = "type")]
    command_type: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
    input: Option<String>,
}

pub fn parse_terminal_client_text(bytes: &[u8]) -> TerminalClientCommand {
    match serde_json::from_slice::<RawTerminalClientCommand>(bytes) {
        Ok(command) => match command.command_type.as_deref() {
            Some("resize") => match (command.cols, command.rows) {
                (Some(cols), Some(rows)) if cols > 0 && rows > 0 => {
                    TerminalClientCommand::Resize { cols, rows }
                }
                _ => TerminalClientCommand::Ignore,
            },
            Some("input") => command
                .input
                .filter(|input| !input.is_empty())
                .map(|input| TerminalClientCommand::Input(input.into_bytes()))
                .unwrap_or(TerminalClientCommand::Ignore),
            _ => TerminalClientCommand::Ignore,
        },
        Err(_) => TerminalClientCommand::Input(bytes.to_vec()),
    }
}

#[derive(Debug, Clone)]
pub struct TungsteniteTerminalConnector {
    endpoint: String,
    token: SharedAgentToken,
    headers: Vec<HeaderPair>,
    custom_dns: String,
}

impl TungsteniteTerminalConnector {
    pub fn from_config(config: &AgentConfig) -> Self {
        Self::from_config_with_token(config, SharedAgentToken::new(config.token.clone()))
    }

    pub fn from_config_with_token(config: &AgentConfig, token: SharedAgentToken) -> Self {
        Self {
            endpoint: config.endpoint.clone(),
            token,
            headers: access_headers(config),
            custom_dns: config.custom_dns.clone(),
        }
    }
}

impl TerminalConnector for TungsteniteTerminalConnector {
    fn start_terminal(
        &self,
        request_id: &str,
        remote_control_disabled: bool,
    ) -> Result<(), TerminalError> {
        let url = build_terminal_ws_url(&self.endpoint, &self.token.get(), request_id)?;
        let mut request = url.into_client_request().map_err(|error| {
            TerminalError::Transport(TransportError::RequestFailed(error.to_string()))
        })?;
        for (name, value) in &self.headers {
            let header_name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
            let header_value = HeaderValue::from_str(value)
                .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
            request.headers_mut().insert(header_name, header_value);
        }

        let (mut socket, _response) = connect_websocket_request(request, &self.custom_dns)?;
        println!(
            "{}",
            smoke_event_line("terminal_session_started", &[("request_id", request_id)])
        );
        if remote_control_disabled {
            socket
                .send(Message::Text(WEB_SSH_DISABLED_MESSAGE.to_string().into()))
                .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
            return Ok(());
        }

        run_terminal_session(socket)
    }
}

#[cfg(target_os = "linux")]
fn run_terminal_session(socket: WebSocket<MaybeTlsStream<TcpStream>>) -> Result<(), TerminalError> {
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    use std::io::{Read, Write};
    use std::sync::Mutex;

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| TerminalError::Io(std::io::Error::new(ErrorKind::Other, error)))?;

    let shell = find_shell().ok_or_else(|| {
        TerminalError::Io(std::io::Error::new(
            ErrorKind::NotFound,
            "no supported shell found among zsh, bash, sh",
        ))
    })?;
    let mut command = CommandBuilder::new(shell);
    command.arg("-i");
    command.env("TERM", "xterm-256color");
    command.env("LANG", "C.UTF-8");
    command.env("LC_ALL", "C.UTF-8");
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| TerminalError::Io(std::io::Error::new(ErrorKind::Other, error)))?;
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| TerminalError::Io(std::io::Error::new(ErrorKind::Other, error)))?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|error| TerminalError::Io(std::io::Error::new(ErrorKind::Other, error)))?;
    let master = pair.master;
    let socket = Arc::new(Mutex::new(socket));
    let output_socket = Arc::clone(&socket);

    let output_thread = thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        loop {
            let count = match reader.read(&mut buffer) {
                Ok(0) => return,
                Ok(count) => count,
                Err(_) => return,
            };
            let mut socket = match output_socket.lock() {
                Ok(socket) => socket,
                Err(_) => return,
            };
            if socket
                .send(Message::Binary(buffer[..count].to_vec().into()))
                .is_err()
            {
                return;
            }
        }
    });

    loop {
        let message = {
            let mut socket = socket.lock().map_err(|_| {
                std::io::Error::new(ErrorKind::Other, "terminal socket lock poisoned")
            })?;
            set_websocket_read_timeout(&mut socket, Some(Duration::from_millis(100)))?;
            match socket.read() {
                Ok(message) => Some(message),
                Err(tungstenite::Error::Io(error))
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    None
                }
                Err(error) => {
                    shutdown_terminal(&mut writer, &mut child);
                    let _ = output_thread.join();
                    return Err(TerminalError::Transport(TransportError::RequestFailed(
                        error.to_string(),
                    )));
                }
            }
        };

        match message {
            Some(Message::Text(text)) => match parse_terminal_client_text(text.as_bytes()) {
                TerminalClientCommand::Input(input) => {
                    writer.write_all(&input)?;
                    writer.flush()?;
                }
                TerminalClientCommand::Resize { cols, rows } => {
                    master
                        .resize(PtySize {
                            rows,
                            cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        })
                        .map_err(|error| {
                            TerminalError::Io(std::io::Error::new(ErrorKind::Other, error))
                        })?;
                }
                TerminalClientCommand::Ignore => {}
            },
            Some(Message::Binary(bytes)) => {
                writer.write_all(&bytes)?;
                writer.flush()?;
            }
            Some(Message::Close(_)) => {
                shutdown_terminal(&mut writer, &mut child);
                let _ = output_thread.join();
                return Ok(());
            }
            Some(_) | None => {}
        }
    }
}

#[cfg(target_os = "linux")]
fn find_shell() -> Option<String> {
    std::env::var("SHELL")
        .ok()
        .filter(|shell| std::path::Path::new(shell).is_file())
        .or_else(|| {
            ["zsh", "bash", "sh"]
                .iter()
                .find_map(|shell| find_in_path(shell))
        })
}

#[cfg(target_os = "linux")]
fn find_in_path(binary: &str) -> Option<String> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|path| path.join(binary))
            .find(|path| path.is_file())
            .map(|path| path.to_string_lossy().to_string())
    })
}

#[cfg(target_os = "linux")]
fn shutdown_terminal(
    writer: &mut Box<dyn std::io::Write + Send>,
    child: &mut Box<dyn portable_pty::Child + Send + Sync>,
) {
    let _ = writer.write_all(&[3]);
    thread::sleep(Duration::from_millis(50));
    let _ = writer.write_all(&[4]);
    thread::sleep(Duration::from_millis(50));
    let _ = writer.write_all(b"exit\n");
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(not(target_os = "linux"))]
fn run_terminal_session(
    mut socket: WebSocket<MaybeTlsStream<TcpStream>>,
) -> Result<(), TerminalError> {
    let _ = socket.send(Message::Text(
        "Terminal is supported on Linux only.".to_string().into(),
    ));
    Err(TerminalError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn set_websocket_read_timeout(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    timeout: Option<Duration>,
) -> Result<(), TerminalError> {
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => stream.set_read_timeout(timeout),
        MaybeTlsStream::Rustls(stream) => stream.sock.set_read_timeout(timeout),
        _ => Ok(()),
    }
    .map_err(TerminalError::Io)
}
