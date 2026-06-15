use crate::ktp::{FrameLeg, FrameType, KtpFrame};
use crate::transport::TransportError;
use crate::tunnel_control::{SelectedTunnelRule, TunnelRuleStateSink};
use crate::tunnel_data::{TunnelDataReadySource, TunnelDataReadyState};
use crate::tunnel_session::{
    decode_session_open_payload, encode_session_accept_payload, encode_session_error_payload,
    encode_session_open_payload, TunnelSessionErrorPayload, TunnelSessionOpenPayload,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

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

pub struct TunnelTcpRuntime {
    rule_state: SharedTunnelRuleState,
    outbound: Arc<Mutex<VecDeque<KtpFrame>>>,
    sessions: Arc<Mutex<HashMap<u64, TcpTunnelSession>>>,
    listeners: HashMap<u64, TcpListenerHandle>,
    next_session_id: Arc<AtomicU64>,
}

struct TcpTunnelSession {
    to_target: mpsc::Sender<Vec<u8>>,
}

struct TcpListenerHandle {
    spec: TunnelTcpListenerSpec,
    stop: Arc<AtomicBool>,
}

impl TunnelTcpRuntime {
    pub fn new(rule_state: SharedTunnelRuleState) -> Self {
        Self {
            rule_state,
            outbound: Arc::new(Mutex::new(VecDeque::new())),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            listeners: HashMap::new(),
            next_session_id: Arc::new(AtomicU64::new(initial_session_id())),
        }
    }

    pub fn refresh_listeners(&mut self) -> Result<(), TransportError> {
        let plan = self.rule_state.tcp_listener_plan();
        let desired_rule_ids = plan.iter().map(|spec| spec.rule_id).collect::<HashSet<_>>();

        self.listeners.retain(|rule_id, handle| {
            let should_keep = desired_rule_ids.contains(rule_id)
                && plan
                    .iter()
                    .find(|spec| spec.rule_id == *rule_id)
                    .map(|spec| same_listener_endpoint(spec, &handle.spec))
                    .unwrap_or(false);
            if !should_keep {
                handle.stop.store(true, Ordering::SeqCst);
            }
            should_keep
        });

        for spec in plan {
            if self.listeners.contains_key(&spec.rule_id) {
                continue;
            }
            let stop = Arc::new(AtomicBool::new(false));
            start_tcp_listener(
                spec.clone(),
                Arc::clone(&self.outbound),
                Arc::clone(&self.sessions),
                Arc::clone(&self.next_session_id),
                Arc::clone(&stop),
            )?;
            self.listeners
                .insert(spec.rule_id, TcpListenerHandle { spec, stop });
        }
        Ok(())
    }

    pub fn active_session_count(&self) -> usize {
        self.sessions
            .lock()
            .map(|sessions| sessions.len())
            .unwrap_or(0)
    }

    fn handle_egress_open(&mut self, frame: KtpFrame) -> Result<Vec<KtpFrame>, TransportError> {
        let open = match decode_session_open_payload(&frame.payload) {
            Ok(open) => open,
            Err(error) => {
                return Ok(vec![session_error_frame(
                    frame.session_id,
                    FrameLeg::Egress,
                    0,
                    "protocol_error",
                    &error.to_string(),
                )?]);
            }
        };
        let Some(rule) = self.find_egress_rule(open.rule_id) else {
            return Ok(vec![session_error_frame(
                frame.session_id,
                FrameLeg::Egress,
                open.rule_id,
                "rule_not_ready",
                "egress rule is not ready",
            )?]);
        };

        let target = tcp_target_addr(&rule.target_host, rule.target_port);
        let stream = match TcpStream::connect(&target) {
            Ok(stream) => stream,
            Err(error) => {
                return Ok(vec![session_error_frame(
                    frame.session_id,
                    FrameLeg::Egress,
                    rule.id,
                    "connect_failed",
                    &error.to_string(),
                )?]);
            }
        };
        let reader = stream
            .try_clone()
            .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
        let writer = stream;
        let (to_target, from_runtime) = mpsc::channel::<Vec<u8>>();
        let outbound = Arc::clone(&self.outbound);
        let sessions = Arc::clone(&self.sessions);
        let session_id = frame.session_id;
        let rule_id = rule.id;
        thread::spawn(move || write_tcp_session(writer, from_runtime));
        thread::spawn(move || {
            read_tcp_session(
                reader,
                outbound,
                sessions,
                session_id,
                rule_id,
                FrameLeg::Egress,
            )
        });
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.insert(frame.session_id, TcpTunnelSession { to_target });
        }

        Ok(vec![KtpFrame {
            frame_type: FrameType::SessionAccept,
            leg: FrameLeg::Egress,
            flags: 0,
            session_id: frame.session_id,
            payload: encode_session_accept_payload(rule.id),
        }])
    }

    fn find_egress_rule(&self, rule_id: u64) -> Option<SelectedTunnelRule> {
        self.rule_state.snapshot().rules.into_iter().find(|rule| {
            rule.id == rule_id
                && rule.enabled
                && rule.protocol.trim().eq_ignore_ascii_case("tcp")
                && matches!(rule.role.trim(), "egress" | "both")
        })
    }
}

impl TunnelSessionRuntime for TunnelTcpRuntime {
    fn handle_server_frame(&mut self, frame: KtpFrame) -> Result<Vec<KtpFrame>, TransportError> {
        match frame.frame_type {
            FrameType::SessionOpen if frame.leg == FrameLeg::Egress => {
                self.handle_egress_open(frame)
            }
            FrameType::SessionData => {
                if let Ok(sessions) = self.sessions.lock() {
                    if let Some(session) = sessions.get(&frame.session_id) {
                        let _ = session.to_target.send(frame.payload);
                    }
                }
                Ok(Vec::new())
            }
            FrameType::SessionClose | FrameType::SessionError => {
                if let Ok(mut sessions) = self.sessions.lock() {
                    sessions.remove(&frame.session_id);
                }
                Ok(Vec::new())
            }
            _ => Ok(Vec::new()),
        }
    }

    fn next_client_frame(&mut self) -> Result<Option<KtpFrame>, TransportError> {
        self.refresh_listeners()?;
        Ok(self
            .outbound
            .lock()
            .ok()
            .and_then(|mut frames| frames.pop_front()))
    }
}

fn tcp_target_addr(host: &str, port: u16) -> String {
    let host = host.trim();
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

fn same_listener_endpoint(left: &TunnelTcpListenerSpec, right: &TunnelTcpListenerSpec) -> bool {
    left.listen_address.trim() == right.listen_address.trim()
        && left.listen_port == right.listen_port
        && left.source_allowlist.trim() == right.source_allowlist.trim()
}

fn initial_session_id() -> u64 {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(1);
    seed.max(1)
}

fn write_tcp_session(mut stream: TcpStream, incoming: mpsc::Receiver<Vec<u8>>) {
    while let Ok(payload) = incoming.recv() {
        if stream.write_all(&payload).is_err() {
            break;
        }
    }
}

fn start_tcp_listener(
    spec: TunnelTcpListenerSpec,
    outbound: Arc<Mutex<VecDeque<KtpFrame>>>,
    sessions: Arc<Mutex<HashMap<u64, TcpTunnelSession>>>,
    next_session_id: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
) -> Result<(), TransportError> {
    let listener = TcpListener::bind(tcp_target_addr(&spec.listen_address, spec.listen_port))
        .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
    thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    if stream.set_nonblocking(false).is_err() {
                        continue;
                    }
                    handle_ingress_stream(
                        spec.clone(),
                        stream,
                        Arc::clone(&outbound),
                        Arc::clone(&sessions),
                        Arc::clone(&next_session_id),
                    );
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(std::time::Duration::from_millis(25));
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });
    Ok(())
}

fn handle_ingress_stream(
    spec: TunnelTcpListenerSpec,
    stream: TcpStream,
    outbound: Arc<Mutex<VecDeque<KtpFrame>>>,
    sessions: Arc<Mutex<HashMap<u64, TcpTunnelSession>>>,
    next_session_id: Arc<AtomicU64>,
) {
    let source_addr = stream
        .peer_addr()
        .map(|addr| addr.to_string())
        .unwrap_or_default();
    if !source_addr_allowed(&source_addr, &spec.source_allowlist) {
        return;
    }

    let session_id = next_session_id.fetch_add(1, Ordering::SeqCst);
    let Ok(payload) = encode_session_open_payload(&TunnelSessionOpenPayload {
        rule_id: spec.rule_id,
        listen_host: spec.listen_address.clone(),
        listen_port: spec.listen_port,
        source_addr,
    }) else {
        return;
    };
    push_outbound_frame(
        &outbound,
        KtpFrame {
            frame_type: FrameType::SessionOpen,
            leg: FrameLeg::Ingress,
            flags: 0,
            session_id,
            payload,
        },
    );

    let Ok(reader) = stream.try_clone() else {
        return;
    };
    let writer = stream;
    let (to_source, from_runtime) = mpsc::channel::<Vec<u8>>();
    if let Ok(mut sessions) = sessions.lock() {
        sessions.insert(
            session_id,
            TcpTunnelSession {
                to_target: to_source,
            },
        );
    }
    let rule_id = spec.rule_id;
    let reader_outbound = Arc::clone(&outbound);
    thread::spawn(move || write_tcp_session(writer, from_runtime));
    thread::spawn(move || {
        read_tcp_session(
            reader,
            reader_outbound,
            sessions,
            session_id,
            rule_id,
            FrameLeg::Ingress,
        )
    });
}

fn read_tcp_session(
    mut stream: TcpStream,
    outbound: Arc<Mutex<VecDeque<KtpFrame>>>,
    sessions: Arc<Mutex<HashMap<u64, TcpTunnelSession>>>,
    session_id: u64,
    rule_id: u64,
    leg: FrameLeg,
) {
    let mut buffer = [0u8; 16 * 1024];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => {
                push_outbound_frame(
                    &outbound,
                    KtpFrame {
                        frame_type: FrameType::SessionClose,
                        leg,
                        flags: 0,
                        session_id,
                        payload: Vec::new(),
                    },
                );
                remove_tcp_session(&sessions, session_id);
                return;
            }
            Ok(read) => push_outbound_frame(
                &outbound,
                KtpFrame {
                    frame_type: FrameType::SessionData,
                    leg,
                    flags: 0,
                    session_id,
                    payload: buffer[..read].to_vec(),
                },
            ),
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => {
                if let Ok(frame) = session_error_frame(
                    session_id,
                    leg,
                    rule_id,
                    "target_read_failed",
                    &error.to_string(),
                ) {
                    push_outbound_frame(&outbound, frame);
                }
                remove_tcp_session(&sessions, session_id);
                return;
            }
        }
    }
}

fn remove_tcp_session(sessions: &Arc<Mutex<HashMap<u64, TcpTunnelSession>>>, session_id: u64) {
    if let Ok(mut sessions) = sessions.lock() {
        sessions.remove(&session_id);
    }
}

fn push_outbound_frame(outbound: &Arc<Mutex<VecDeque<KtpFrame>>>, frame: KtpFrame) {
    if let Ok(mut frames) = outbound.lock() {
        frames.push_back(frame);
    }
}

fn session_error_frame(
    session_id: u64,
    leg: FrameLeg,
    rule_id: u64,
    code: &str,
    message: &str,
) -> Result<KtpFrame, TransportError> {
    let payload = encode_session_error_payload(&TunnelSessionErrorPayload {
        rule_id,
        code: code.to_string(),
        message: message.to_string(),
    })
    .map_err(|error| TransportError::RequestFailed(error.to_string()))?;
    Ok(KtpFrame {
        frame_type: FrameType::SessionError,
        leg,
        flags: 0,
        session_id,
        payload,
    })
}

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
