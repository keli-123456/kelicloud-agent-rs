use crate::ktp::{FrameLeg, FrameType, KtpFrame};
use crate::transport::TransportError;
use crate::tunnel_control::{SelectedTunnelRule, TunnelRuleStateSink};
use crate::tunnel_data::{TunnelDataReadySource, TunnelDataReadyState, TunnelDataRuleFailure};
use crate::tunnel_preflight::{
    tunnel_preflight_status, validate_tunnel_tcp_rule_for_side, TunnelPreflightIssue,
    TunnelPreflightIssueCode, TunnelPreflightSide, TunnelTcpRulePreflightInput,
};
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
    inner: Arc<Mutex<TunnelRuleSharedState>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TunnelRuleSnapshot {
    pub revision: String,
    pub rules: Vec<SelectedTunnelRule>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TunnelListenerHealthStatus {
    Running,
    StartFailed,
    Stopped,
    RuntimeError,
}

impl TunnelListenerHealthStatus {
    fn ready_status(&self) -> &'static str {
        match self {
            Self::Running => "listener_running",
            Self::StartFailed => "listener_start_failed",
            Self::Stopped => "listener_stopped",
            Self::RuntimeError => "listener_runtime_error",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TunnelListenerHealth {
    pub spec: TunnelTcpListenerSpec,
    pub status: TunnelListenerHealthStatus,
    pub error: String,
}

impl TunnelListenerHealth {
    fn running(spec: TunnelTcpListenerSpec) -> Self {
        Self {
            spec,
            status: TunnelListenerHealthStatus::Running,
            error: String::new(),
        }
    }

    fn failure(
        spec: TunnelTcpListenerSpec,
        status: TunnelListenerHealthStatus,
        error: &str,
    ) -> Self {
        Self {
            spec,
            status,
            error: error.trim().to_string(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TunnelRuleSharedState {
    revision: String,
    rules: Vec<SelectedTunnelRule>,
    active_listeners: Vec<TunnelTcpListenerSpec>,
    listener_health: Vec<TunnelListenerHealth>,
}

impl SharedTunnelRuleState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(TunnelRuleSharedState {
                revision: String::new(),
                rules: Vec::new(),
                active_listeners: Vec::new(),
                listener_health: Vec::new(),
            })),
        }
    }

    pub fn snapshot(&self) -> TunnelRuleSnapshot {
        self.inner
            .lock()
            .map(|state| TunnelRuleSnapshot {
                revision: state.revision.clone(),
                rules: state.rules.clone(),
            })
            .unwrap_or_else(|_| empty_tunnel_rule_snapshot())
    }

    pub fn tcp_listener_plan(&self) -> Vec<TunnelTcpListenerSpec> {
        build_tcp_listener_plan(&self.snapshot().rules)
    }

    fn ready_snapshot(
        &self,
    ) -> (
        TunnelRuleSnapshot,
        Vec<TunnelTcpListenerSpec>,
        Vec<TunnelListenerHealth>,
    ) {
        self.inner
            .lock()
            .map(|state| {
                (
                    TunnelRuleSnapshot {
                        revision: state.revision.clone(),
                        rules: state.rules.clone(),
                    },
                    state.active_listeners.clone(),
                    state.listener_health.clone(),
                )
            })
            .unwrap_or_else(|_| (empty_tunnel_rule_snapshot(), Vec::new(), Vec::new()))
    }

    pub fn set_listener_running(&self, spec: TunnelTcpListenerSpec) {
        self.set_listener_health(TunnelListenerHealth::running(spec));
    }

    pub fn set_listener_start_failed(&self, spec: TunnelTcpListenerSpec, error: &str) {
        self.set_listener_health(TunnelListenerHealth::failure(
            spec,
            TunnelListenerHealthStatus::StartFailed,
            error,
        ));
    }

    pub fn set_listener_stopped(&self, spec: TunnelTcpListenerSpec, error: &str) {
        self.set_listener_health(TunnelListenerHealth::failure(
            spec,
            TunnelListenerHealthStatus::Stopped,
            error,
        ));
    }

    pub fn set_listener_runtime_error(&self, spec: TunnelTcpListenerSpec, error: &str) {
        self.set_listener_health(TunnelListenerHealth::failure(
            spec,
            TunnelListenerHealthStatus::RuntimeError,
            error,
        ));
    }

    fn set_listener_health(&self, health: TunnelListenerHealth) {
        if let Ok(mut state) = self.inner.lock() {
            state
                .listener_health
                .retain(|existing| !same_listener_identity(&existing.spec, &health.spec));
            state.listener_health.push(health);
            sort_listener_health(&mut state.listener_health);
        }
    }

    fn retain_listener_health_for_specs(&self, specs: &[TunnelTcpListenerSpec]) {
        if let Ok(mut state) = self.inner.lock() {
            retain_listener_health_for_specs(&mut state.listener_health, specs);
        }
    }

    fn set_active_tcp_listeners(&self, mut listeners: Vec<TunnelTcpListenerSpec>) {
        listeners.sort_by_key(|listener| listener.rule_id);
        if let Ok(mut state) = self.inner.lock() {
            state.active_listeners = listeners.clone();
            state.listener_health.retain(|health| {
                health.status != TunnelListenerHealthStatus::Running
                    || listeners
                        .iter()
                        .any(|listener| same_listener_identity(&health.spec, listener))
            });
            for listener in listeners {
                if !state
                    .listener_health
                    .iter()
                    .any(|health| same_listener_identity(&health.spec, &listener))
                {
                    state
                        .listener_health
                        .push(TunnelListenerHealth::running(listener));
                }
            }
            sort_listener_health(&mut state.listener_health);
        }
    }
}

fn empty_tunnel_rule_snapshot() -> TunnelRuleSnapshot {
    TunnelRuleSnapshot {
        revision: String::new(),
        rules: Vec::new(),
    }
}

impl Default for SharedTunnelRuleState {
    fn default() -> Self {
        Self::new()
    }
}

impl TunnelRuleStateSink for SharedTunnelRuleState {
    fn update_rules(&self, revision: &str, rules: &[SelectedTunnelRule]) {
        let listener_plan = build_tcp_listener_plan(rules);
        if let Ok(mut state) = self.inner.lock() {
            state.revision = revision.trim().to_string();
            state.rules = rules.to_vec();
            retain_listener_health_for_specs(&mut state.listener_health, &listener_plan);
            state.active_listeners.retain(|listener| {
                listener_plan
                    .iter()
                    .any(|spec| same_listener_identity(listener, spec))
            });
        }
    }
}

impl TunnelDataReadySource for SharedTunnelRuleState {
    fn current_ready(&self) -> TunnelDataReadyState {
        let (snapshot, active_listeners, listener_health) = self.ready_snapshot();
        build_tunnel_ready_state_with_listener_health(
            &snapshot.revision,
            &snapshot.rules,
            &active_listeners,
            &listener_health,
        )
    }
}

pub trait TunnelSessionRuntime {
    fn tick(&mut self) -> Result<(), TransportError> {
        Ok(())
    }

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
    listener_event_tx: mpsc::Sender<TcpListenerRuntimeEvent>,
    listener_event_rx: mpsc::Receiver<TcpListenerRuntimeEvent>,
    next_session_id: Arc<AtomicU64>,
}

struct TcpTunnelSession {
    to_target: mpsc::Sender<Vec<u8>>,
}

struct TcpListenerHandle {
    spec: TunnelTcpListenerSpec,
    stop: Arc<AtomicBool>,
}

#[derive(Clone, Debug)]
struct TcpListenerRuntimeEvent {
    spec: TunnelTcpListenerSpec,
}

impl TunnelTcpRuntime {
    pub fn new(rule_state: SharedTunnelRuleState) -> Self {
        let (listener_event_tx, listener_event_rx) = mpsc::channel();
        Self {
            rule_state,
            outbound: Arc::new(Mutex::new(VecDeque::new())),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            listeners: HashMap::new(),
            listener_event_tx,
            listener_event_rx,
            next_session_id: Arc::new(AtomicU64::new(initial_session_id())),
        }
    }

    pub fn refresh_listeners(&mut self) -> Result<(), TransportError> {
        let terminal_specs = self.drain_listener_runtime_events();
        let plan = self.rule_state.tcp_listener_plan();
        self.rule_state.retain_listener_health_for_specs(&plan);
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
        self.sync_active_listener_snapshot();

        for spec in plan {
            if self.listeners.contains_key(&spec.rule_id) {
                continue;
            }
            if terminal_specs
                .iter()
                .any(|terminal| same_listener_identity(terminal, &spec))
            {
                continue;
            }
            let stop = Arc::new(AtomicBool::new(false));
            match start_tcp_listener(
                spec.clone(),
                self.rule_state.clone(),
                self.listener_event_tx.clone(),
                Arc::clone(&self.outbound),
                Arc::clone(&self.sessions),
                Arc::clone(&self.next_session_id),
                Arc::clone(&stop),
            ) {
                Ok(()) => {
                    self.rule_state.set_listener_running(spec.clone());
                    self.listeners
                        .insert(spec.rule_id, TcpListenerHandle { spec, stop });
                }
                Err(error) => {
                    self.rule_state
                        .set_listener_start_failed(spec.clone(), &error.to_string());
                }
            }
        }
        self.sync_active_listener_snapshot();
        Ok(())
    }

    pub fn active_session_count(&self) -> usize {
        self.sessions
            .lock()
            .map(|sessions| sessions.len())
            .unwrap_or(0)
    }

    fn sync_active_listener_snapshot(&self) {
        self.rule_state.set_active_tcp_listeners(
            self.listeners
                .values()
                .map(|handle| handle.spec.clone())
                .collect(),
        );
    }

    fn drain_listener_runtime_events(&mut self) -> Vec<TunnelTcpListenerSpec> {
        let mut terminal_specs = Vec::new();
        while let Ok(event) = self.listener_event_rx.try_recv() {
            let should_remove = self
                .listeners
                .get(&event.spec.rule_id)
                .map(|handle| same_listener_identity(&handle.spec, &event.spec))
                .unwrap_or(false);
            if !should_remove {
                continue;
            }
            if let Some(handle) = self.listeners.remove(&event.spec.rule_id) {
                handle.stop.store(true, Ordering::SeqCst);
            }
            terminal_specs.push(event.spec);
        }
        if !terminal_specs.is_empty() {
            self.sync_active_listener_snapshot();
        }
        terminal_specs
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
                "runtime_unavailable",
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
                    "target_connect_failed",
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
    fn tick(&mut self) -> Result<(), TransportError> {
        self.refresh_listeners()
    }

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

fn same_listener_identity(left: &TunnelTcpListenerSpec, right: &TunnelTcpListenerSpec) -> bool {
    left.rule_id == right.rule_id && same_listener_endpoint(left, right)
}

fn retain_listener_health_for_specs(
    listener_health: &mut Vec<TunnelListenerHealth>,
    specs: &[TunnelTcpListenerSpec],
) {
    listener_health.retain(|health| {
        specs
            .iter()
            .any(|spec| same_listener_identity(&health.spec, spec))
    });
}

fn sort_listener_health(listener_health: &mut [TunnelListenerHealth]) {
    listener_health.sort_by(|left, right| {
        left.spec
            .rule_id
            .cmp(&right.spec.rule_id)
            .then_with(|| left.spec.listen_address.cmp(&right.spec.listen_address))
            .then_with(|| left.spec.listen_port.cmp(&right.spec.listen_port))
            .then_with(|| left.spec.source_allowlist.cmp(&right.spec.source_allowlist))
    });
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
    rule_state: SharedTunnelRuleState,
    listener_events: mpsc::Sender<TcpListenerRuntimeEvent>,
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
        let mut terminal_error = String::new();
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
                Err(error) => {
                    terminal_error = error.to_string();
                    break;
                }
            }
        }
        if !stop.load(Ordering::SeqCst) {
            let terminal_spec = spec.clone();
            if terminal_error.trim().is_empty() {
                rule_state
                    .set_listener_stopped(terminal_spec.clone(), "listener stopped unexpectedly");
            } else {
                rule_state.set_listener_runtime_error(terminal_spec.clone(), &terminal_error);
            }
            let _ = listener_events.send(TcpListenerRuntimeEvent {
                spec: terminal_spec,
            });
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

pub fn build_tunnel_ready_state(
    revision: &str,
    rules: &[SelectedTunnelRule],
) -> TunnelDataReadyState {
    build_tunnel_ready_state_with_listener_health(revision, rules, &[], &[])
}

fn build_tunnel_ready_state_with_listener_health(
    revision: &str,
    rules: &[SelectedTunnelRule],
    active_listeners: &[TunnelTcpListenerSpec],
    listener_health: &[TunnelListenerHealth],
) -> TunnelDataReadyState {
    let mut ready = TunnelDataReadyState::empty(revision);
    for rule in rules {
        if !rule.enabled || rule.protocol.trim().to_ascii_lowercase() != "tcp" {
            continue;
        }
        match rule.role.trim().to_ascii_lowercase().as_str() {
            "ingress" => apply_ready_side(
                rule,
                TunnelPreflightSide::Ingress,
                active_listeners,
                listener_health,
                &mut ready,
            ),
            "egress" => apply_ready_side(
                rule,
                TunnelPreflightSide::Egress,
                active_listeners,
                listener_health,
                &mut ready,
            ),
            "both" => {
                apply_ready_side(
                    rule,
                    TunnelPreflightSide::Ingress,
                    active_listeners,
                    listener_health,
                    &mut ready,
                );
                apply_ready_side(
                    rule,
                    TunnelPreflightSide::Egress,
                    active_listeners,
                    listener_health,
                    &mut ready,
                );
            }
            _ => {}
        }
    }
    ready.ingress_rule_ids.sort_unstable();
    ready.ingress_rule_ids.dedup();
    ready.egress_rule_ids.sort_unstable();
    ready.egress_rule_ids.dedup();
    ready
}

fn apply_ready_side(
    rule: &SelectedTunnelRule,
    side: TunnelPreflightSide,
    active_listeners: &[TunnelTcpListenerSpec],
    listener_health: &[TunnelListenerHealth],
    ready: &mut TunnelDataReadyState,
) {
    let input = TunnelTcpRulePreflightInput {
        rule_id: rule.id,
        listen_address: rule.listen_address.clone(),
        listen_port: rule.listen_port,
        target_host: rule.target_host.clone(),
        target_port: rule.target_port,
        source_allowlist: rule.source_allowlist.clone(),
    };
    let issues = filter_runtime_owned_listener_bind_failure(
        rule,
        side,
        validate_tunnel_tcp_rule_for_side(&input, side),
        active_listeners,
    );
    let has_unsupported_os = issues
        .iter()
        .any(|issue| issue.code == TunnelPreflightIssueCode::UnsupportedOs);
    let mut blocked = false;

    if side == TunnelPreflightSide::Ingress
        && !has_unsupported_os
        && push_listener_health_failure(rule, active_listeners, listener_health, ready)
    {
        blocked = true;
    }
    for issue in issues {
        blocked = true;
        push_preflight_failure_once(ready, issue);
    }
    if blocked {
        return;
    }

    match side {
        TunnelPreflightSide::Ingress => ready.ingress_rule_ids.push(rule.id),
        TunnelPreflightSide::Egress => ready.egress_rule_ids.push(rule.id),
    }
}

fn filter_runtime_owned_listener_bind_failure(
    rule: &SelectedTunnelRule,
    side: TunnelPreflightSide,
    issues: Vec<TunnelPreflightIssue>,
    active_listeners: &[TunnelTcpListenerSpec],
) -> Vec<TunnelPreflightIssue> {
    let expected = listener_spec_for_rule(rule);
    if side != TunnelPreflightSide::Ingress
        || !active_listeners
            .iter()
            .any(|listener| same_listener_identity(listener, &expected))
    {
        return issues;
    }
    issues
        .into_iter()
        .filter(|issue| issue.code != TunnelPreflightIssueCode::ListenBindFailed)
        .collect()
}

fn push_listener_health_failure(
    rule: &SelectedTunnelRule,
    active_listeners: &[TunnelTcpListenerSpec],
    listener_health: &[TunnelListenerHealth],
    ready: &mut TunnelDataReadyState,
) -> bool {
    let expected = listener_spec_for_rule(rule);
    let active = active_listeners
        .iter()
        .any(|listener| same_listener_identity(listener, &expected));
    let health = listener_health
        .iter()
        .find(|health| same_listener_identity(&health.spec, &expected));
    match health {
        Some(health) if health.status == TunnelListenerHealthStatus::Running && active => false,
        Some(health) if health.status == TunnelListenerHealthStatus::Running => {
            push_listener_failure_once(
                ready,
                rule.id,
                TunnelListenerHealthStatus::Stopped.ready_status(),
                &format!(
                    "listener is not running on {}",
                    tcp_target_addr(&expected.listen_address, expected.listen_port)
                ),
            );
            true
        }
        Some(health) => {
            let error = health.error.trim();
            push_listener_failure_once(
                ready,
                rule.id,
                health.status.ready_status(),
                if error.is_empty() {
                    "listener is not running"
                } else {
                    error
                },
            );
            true
        }
        None => {
            push_listener_failure_once(
                ready,
                rule.id,
                TunnelListenerHealthStatus::Stopped.ready_status(),
                &format!(
                    "listener is not running on {}",
                    tcp_target_addr(&expected.listen_address, expected.listen_port)
                ),
            );
            true
        }
    }
}

fn push_listener_failure_once(
    ready: &mut TunnelDataReadyState,
    rule_id: u64,
    status: &str,
    error: &str,
) {
    let error = error.trim().to_string();
    if ready.failed_rules.iter().any(|failure| {
        failure.rule_id == rule_id && failure.status == status && failure.error == error
    }) {
        return;
    }
    ready.failed_rules.push(TunnelDataRuleFailure {
        rule_id,
        status: status.to_string(),
        error,
    });
}

fn push_preflight_failure_once(ready: &mut TunnelDataReadyState, issue: TunnelPreflightIssue) {
    let status = tunnel_preflight_status(issue.code).to_string();
    if ready.failed_rules.iter().any(|failure| {
        failure.rule_id == issue.rule_id
            && failure.status == status
            && failure.error == issue.message
    }) {
        return;
    }
    ready.failed_rules.push(TunnelDataRuleFailure {
        rule_id: issue.rule_id,
        status,
        error: issue.message,
    });
}

fn listener_spec_for_rule(rule: &SelectedTunnelRule) -> TunnelTcpListenerSpec {
    TunnelTcpListenerSpec {
        rule_id: rule.id,
        name: rule.name.trim().to_string(),
        listen_address: rule.listen_address.trim().to_string(),
        listen_port: rule.listen_port,
        target_host: rule.target_host.trim().to_string(),
        target_port: rule.target_port,
        source_allowlist: rule.source_allowlist.trim().to_string(),
        max_concurrent_sessions: rule.max_concurrent_sessions,
    }
}

pub fn build_tcp_listener_plan(rules: &[SelectedTunnelRule]) -> Vec<TunnelTcpListenerSpec> {
    let mut listeners = rules
        .iter()
        .filter(|rule| rule.enabled)
        .filter(|rule| rule.protocol.trim().eq_ignore_ascii_case("tcp"))
        .filter(|rule| matches!(rule.role.trim(), "ingress" | "both"))
        .map(listener_spec_for_rule)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcp_runtime_terminal_event_removes_handle_and_delays_restart_for_ready_visibility() {
        if !cfg!(target_os = "linux") {
            return;
        }
        let listen_port = free_tcp_port();
        let state = SharedTunnelRuleState::new();
        let mut rule = selected_rule(501);
        rule.listen_port = listen_port;
        state.update_rules("rev-terminal", &[rule.clone()]);
        let spec = listener_spec_for_rule(&rule);
        let mut runtime = TunnelTcpRuntime::new(state.clone());
        runtime.listeners.insert(
            spec.rule_id,
            TcpListenerHandle {
                spec: spec.clone(),
                stop: Arc::new(AtomicBool::new(false)),
            },
        );
        runtime.sync_active_listener_snapshot();
        state.set_listener_runtime_error(spec.clone(), "accept failed: socket closed");
        runtime
            .listener_event_tx
            .send(TcpListenerRuntimeEvent { spec: spec.clone() })
            .expect("terminal event should send");

        runtime
            .refresh_listeners()
            .expect("terminal event should be drained");
        let failed = state.current_ready();

        assert!(
            !runtime.listeners.contains_key(&spec.rule_id),
            "matching terminal event must remove stale listener handle"
        );
        assert!(
            !failed.ingress_rule_ids.contains(&spec.rule_id),
            "terminal listener must not stay active in READY: {:?}",
            failed
        );
        assert!(
            failed.failed_rules.iter().any(|failure| {
                failure.rule_id == spec.rule_id && failure.status == "listener_runtime_error"
            }),
            "runtime failure should stay visible for one READY cycle: {:?}",
            failed.failed_rules
        );
        assert!(
            TcpStream::connect(("127.0.0.1", listen_port)).is_err(),
            "same refresh cycle must not restart the terminal listener"
        );

        runtime
            .refresh_listeners()
            .expect("next refresh should restart listener");
        let recovered = state.current_ready();

        assert!(
            runtime.listeners.contains_key(&spec.rule_id),
            "later refresh should recreate listener handle"
        );
        assert!(
            recovered.ingress_rule_ids.contains(&spec.rule_id),
            "later refresh should recover READY: {:?}",
            recovered
        );
        assert!(
            recovered.failed_rules.iter().all(|failure| {
                failure.rule_id != spec.rule_id || failure.status != "listener_runtime_error"
            }),
            "runtime failure should clear after restart: {:?}",
            recovered.failed_rules
        );
    }

    #[test]
    fn tcp_runtime_terminal_event_for_old_identity_keeps_newer_handle() {
        let state = SharedTunnelRuleState::new();
        let mut old_rule = selected_rule(502);
        old_rule.listen_port = 15020;
        let old_spec = listener_spec_for_rule(&old_rule);
        let mut new_rule = selected_rule(502);
        new_rule.listen_port = 15021;
        new_rule.source_allowlist = "127.0.0.1".to_string();
        state.update_rules("rev-new-listener", &[new_rule.clone()]);
        let new_spec = listener_spec_for_rule(&new_rule);
        let mut runtime = TunnelTcpRuntime::new(state);
        runtime.listeners.insert(
            new_spec.rule_id,
            TcpListenerHandle {
                spec: new_spec.clone(),
                stop: Arc::new(AtomicBool::new(false)),
            },
        );
        runtime.sync_active_listener_snapshot();
        runtime
            .listener_event_tx
            .send(TcpListenerRuntimeEvent { spec: old_spec })
            .expect("terminal event should send");

        runtime
            .refresh_listeners()
            .expect("old terminal event should be ignored");

        let kept = runtime
            .listeners
            .get(&new_spec.rule_id)
            .expect("newer listener handle should remain");
        assert_eq!(kept.spec, new_spec);
    }

    fn selected_rule(id: u64) -> SelectedTunnelRule {
        SelectedTunnelRule {
            id,
            name: format!("rule-{id}"),
            enabled: true,
            protocol: "tcp".to_string(),
            role: "ingress".to_string(),
            ingress_group: "edge".to_string(),
            listen_address: "127.0.0.1".to_string(),
            listen_port: 10000 + id as u16,
            egress_group: "rdp".to_string(),
            target_host: "127.0.0.1".to_string(),
            target_port: 3389,
            source_allowlist: "127.0.0.0/8".to_string(),
            max_concurrent_sessions: 32,
            last_revision: 1,
        }
    }

    fn free_tcp_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind free port");
        listener.local_addr().expect("local addr").port()
    }
}
