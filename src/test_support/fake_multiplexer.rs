use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{Deadline, Gate};
use crate::application::ports::{
    AgentLaunch, AttachTarget, Multiplexer, MultiplexerError, ServerConfig, ServerState,
    SessionSpec,
};
use crate::domain::{
    AgentPane, HotKey, KeyBindings, SessionMetadata, SessionObservation, TmuxAddress,
};

pub struct FakeMultiplexer {
    state: Mutex<MultiplexerState>,
    hooks: Mutex<HashMap<&'static str, CallHook>>,
    typing_gate: Arc<Gate>,
}

type CallHook = Arc<dyn Fn() + Send + Sync>;

struct TypedLine {
    method: &'static str,
    tmux_name: String,
    line: String,
}

struct MultiplexerState {
    socket: PathBuf,
    live: Vec<SessionObservation>,
    specs: Vec<SessionSpec>,
    launches: Vec<AgentLaunch>,
    typed: Vec<TypedLine>,
    pane_pids: HashMap<String, Vec<u32>>,
    caller: Option<String>,
    caller_in_own_pane: bool,
    caller_in_other_tmux: bool,
    captures: HashMap<String, String>,
    failures: HashMap<&'static str, MultiplexerError>,
    failed_create_still_creates: bool,
    attached: Vec<AttachTarget>,
    calls: Vec<String>,
    client_version: String,
    server: Option<ServerState>,
    config_current: bool,
    server_key_bindings: Option<KeyBindings>,
    socket_lost: bool,
}

impl FakeMultiplexer {
    pub const SOCKET: &'static str = "/run/user/1000/ringleader/tmux/tmux.sock";
    pub const STATE_CHANGING: [&'static str; 7] = [
        "create_session",
        "kill_session",
        "rename_session",
        "respawn_agent",
        "type_when_ready",
        "type_if_ready",
        "ensure_shell_window",
    ];
    const TYPING_GATE_LIMIT: Duration = Duration::from_secs(5);

    pub fn new() -> Self {
        Self {
            hooks: Mutex::new(HashMap::new()),
            typing_gate: Arc::new(Gate::open()),
            state: Mutex::new(MultiplexerState {
                socket: PathBuf::from(Self::SOCKET),
                live: Vec::new(),
                specs: Vec::new(),
                launches: Vec::new(),
                typed: Vec::new(),
                pane_pids: HashMap::new(),
                caller: None,
                caller_in_own_pane: false,
                caller_in_other_tmux: false,
                captures: HashMap::new(),
                failures: HashMap::new(),
                failed_create_still_creates: false,
                attached: Vec::new(),
                calls: Vec::new(),
                client_version: "3.6a".to_string(),
                server: Some(ServerState::new("3.6a", 1)),
                config_current: true,
                server_key_bindings: Some(KeyBindings::default()),
                socket_lost: false,
            }),
        }
    }

    pub fn with_live(self, observation: SessionObservation) -> Self {
        self.edit(|state| state.live.push(observation));
        self
    }

    pub fn with_pane_pids(self, tmux_name: &str, pids: &[u32]) -> Self {
        self.edit(|state| state.pane_pids.insert(tmux_name.to_string(), pids.to_vec()));
        self
    }

    pub fn with_caller(self, tmux_name: &str) -> Self {
        self.edit(|state| state.caller = Some(tmux_name.to_string()));
        self
    }

    pub fn called_from_own_pane(self) -> Self {
        self.edit(|state| state.caller_in_own_pane = true);
        self
    }

    pub fn with_capture(self, tmux_name: &str, text: &str) -> Self {
        self.edit(|state| {
            state
                .captures
                .insert(tmux_name.to_string(), text.to_string())
        });
        self
    }

    pub fn failing(self, operation: &'static str, error: MultiplexerError) -> Self {
        self.edit(|state| state.failures.insert(operation, error));
        self
    }

    pub fn on_call(self, operation: &'static str, hook: impl Fn() + Send + Sync + 'static) -> Self {
        self.add_call_hook(operation, hook);
        self
    }

    pub fn add_call_hook(&self, operation: &'static str, hook: impl Fn() + Send + Sync + 'static) {
        self.hooks.lock().unwrap().insert(operation, Arc::new(hook));
    }

    pub fn with_versions(self, client: &str, server: Option<&str>) -> Self {
        self.edit(|state| {
            state.client_version = client.to_string();
            state.server = server.map(|version| ServerState::new(version, 1));
        });
        self
    }

    pub fn with_lost_socket(self) -> Self {
        self.lose_socket();
        self
    }

    pub fn lose_socket(&self) {
        self.edit(|state| state.socket_lost = true);
    }

    pub fn with_drifted_config(self, server_return_key: &str) -> Self {
        self.drift_config(server_return_key);
        self
    }

    pub fn drift_config(&self, server_return_key: &str) {
        self.edit(|state| {
            state.config_current = false;
            state.server_key_bindings = HotKey::parse(server_return_key)
                .map(|return_key| KeyBindings::default().with_return_key(return_key));
        });
    }

    pub fn called_from_other_tmux(self) -> Self {
        self.edit(|state| state.caller_in_other_tmux = true);
        self
    }

    pub fn with_typing_gate(mut self, gate: Arc<Gate>) -> Self {
        self.typing_gate = gate;
        self
    }

    pub fn blocking_typing(mut self) -> Self {
        self.typing_gate = Arc::new(Gate::closed());
        self
    }

    pub fn release_typing(&self) {
        self.typing_gate.open_now();
    }

    pub fn wait_for_typing(&self, tmux_name: &str, limit: Duration) -> bool {
        Deadline::after(limit).wait_until(|| !self.typed_when_ready(tmux_name).is_empty())
    }

    pub fn creating_despite_failure(self) -> Self {
        self.edit(|state| state.failed_create_still_creates = true);
        self
    }

    pub fn fail(&self, operation: &'static str, error: MultiplexerError) {
        self.edit(|state| state.failures.insert(operation, error));
    }

    pub fn replace_live(&self, observation: SessionObservation) {
        self.end_session(observation.tmux_name());
        self.edit(|state| state.live.push(observation));
    }

    pub fn end_session(&self, tmux_name: &str) {
        self.edit(|state| state.live.retain(|live| live.tmux_name() != tmux_name));
    }

    pub fn calls(&self) -> Vec<String> {
        self.edit(|state| state.calls.clone())
    }

    pub fn state_changing_calls(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter(|call| {
                Self::STATE_CHANGING
                    .iter()
                    .any(|name| call.starts_with(name))
            })
            .collect()
    }

    pub fn specs(&self) -> Vec<SessionSpec> {
        self.edit(|state| state.specs.clone())
    }

    pub fn launches(&self) -> Vec<AgentLaunch> {
        self.edit(|state| state.launches.clone())
    }

    pub fn typed_when_ready(&self, tmux_name: &str) -> Vec<String> {
        self.typed_by("type_when_ready", tmux_name)
    }

    pub fn typed_if_ready(&self, tmux_name: &str) -> Vec<String> {
        self.typed_by("type_if_ready", tmux_name)
    }

    fn typed_by(&self, method: &str, tmux_name: &str) -> Vec<String> {
        self.edit(|state| {
            state
                .typed
                .iter()
                .filter(|typed| typed.method == method && typed.tmux_name == tmux_name)
                .map(|typed| typed.line.clone())
                .collect()
        })
    }

    pub fn live_names(&self) -> Vec<String> {
        self.edit(|state| {
            state
                .live
                .iter()
                .map(|live| live.tmux_name().to_string())
                .collect()
        })
    }

    pub fn attached(&self) -> Vec<AttachTarget> {
        self.edit(|state| state.attached.clone())
    }

    fn edit<T>(&self, change: impl FnOnce(&mut MultiplexerState) -> T) -> T {
        change(&mut self.state.lock().unwrap())
    }

    fn pass_typing_gate(&self, tmux_name: &str) -> Result<(), MultiplexerError> {
        match self.typing_gate.wait_up_to(Self::TYPING_GATE_LIMIT) {
            true => Ok(()),
            false => Err(MultiplexerError::AgentNotReady {
                tmux_name: tmux_name.to_string(),
            }),
        }
    }

    fn call(&self, operation: &'static str, detail: &str) -> Result<(), MultiplexerError> {
        let hook = self.hooks.lock().unwrap().get(operation).cloned();
        if let Some(hook) = hook {
            hook();
        }
        self.edit(|state| {
            state.calls.push(format!("{operation} {detail}"));
            match state.failures.get(operation) {
                Some(error) => Err(error.clone()),
                None => Ok(()),
            }
        })
    }
}

impl MultiplexerState {
    fn start_session(&mut self, tmux_name: &str, metadata: &SessionMetadata) {
        let tmux = TmuxAddress::new(tmux_name, &self.socket);
        let observation = SessionObservation::new(tmux, 0)
            .with_agent_pane(AgentPane::live(0))
            .with_metadata(metadata.clone());
        self.live.push(observation);
    }

    fn type_lines(&mut self, method: &'static str, tmux_name: &str, lines: &[String]) {
        self.typed.extend(lines.iter().map(|line| TypedLine {
            method,
            tmux_name: tmux_name.to_string(),
            line: line.clone(),
        }));
    }

    fn find_live(&self, tmux_name: &str) -> Option<&SessionObservation> {
        self.live.iter().find(|live| live.tmux_name() == tmux_name)
    }
}

impl Multiplexer for FakeMultiplexer {
    fn socket(&self) -> PathBuf {
        self.edit(|state| state.socket.clone())
    }

    fn create_session(&self, spec: &SessionSpec) -> Result<PathBuf, MultiplexerError> {
        let result = self.call("create_session", spec.tmux_name());
        self.edit(|state| {
            state.specs.push(spec.clone());
            state.launches.push(spec.launch().clone());
            if result.is_ok() || state.failed_create_still_creates {
                state.start_session(spec.tmux_name(), spec.metadata());
            }
            result.map(|_| state.socket.clone())
        })
    }

    fn kill_session(&self, tmux_name: &str) -> Result<(), MultiplexerError> {
        self.call("kill_session", tmux_name)?;
        self.end_session(tmux_name);
        Ok(())
    }

    fn pane_pids(&self, tmux_name: &str) -> Result<Vec<u32>, MultiplexerError> {
        self.call("pane_pids", tmux_name)?;
        Ok(self.edit(|state| state.pane_pids.get(tmux_name).cloned().unwrap_or_default()))
    }

    fn caller_in_other_tmux(&self) -> bool {
        self.edit(|state| state.caller_in_other_tmux)
    }

    fn caller_session(&self) -> Result<Option<String>, MultiplexerError> {
        Ok(self.edit(|state| state.caller.clone()))
    }

    fn caller_in_own_pane(&self) -> Result<bool, MultiplexerError> {
        Ok(self.edit(|state| state.caller_in_own_pane))
    }

    fn observe_all(&self) -> Result<Vec<SessionObservation>, MultiplexerError> {
        self.call("observe_all", "")?;
        Ok(self.edit(|state| match state.socket_lost {
            true => Vec::new(),
            false => state.live.clone(),
        }))
    }

    fn attach(&self, target: &AttachTarget) -> Result<(), MultiplexerError> {
        self.call("attach", &format!("{target:?}"))?;
        self.edit(|state| state.attached.push(target.clone()));
        Ok(())
    }

    fn capture_pane(&self, tmux_name: &str) -> Result<String, MultiplexerError> {
        self.call("capture_pane", tmux_name)?;
        Ok(self.edit(|state| state.captures.get(tmux_name).cloned().unwrap_or_default()))
    }

    fn rename_session(
        &self,
        old: &str,
        new: &str,
        metadata: &SessionMetadata,
    ) -> Result<(), MultiplexerError> {
        self.call("rename_session", &format!("{old} {new}"))?;
        self.edit(|state| {
            state.live.retain(|live| live.tmux_name() != old);
            state.start_session(new, metadata);
        });
        Ok(())
    }

    fn respawn_agent(&self, tmux_name: &str, launch: &AgentLaunch) -> Result<(), MultiplexerError> {
        self.call("respawn_agent", tmux_name)?;
        self.edit(|state| {
            state.launches.push(launch.clone());
            if let Some(live) = state.find_live(tmux_name).cloned() {
                state.live.retain(|other| other.tmux_name() != tmux_name);
                state.live.push(live.with_agent_pane(AgentPane::live(0)));
            }
        });
        Ok(())
    }

    fn type_when_ready(&self, tmux_name: &str, lines: &[String]) -> Result<(), MultiplexerError> {
        self.call("type_when_ready", tmux_name)?;
        self.pass_typing_gate(tmux_name)?;
        self.edit(|state| state.type_lines("type_when_ready", tmux_name, lines));
        Ok(())
    }

    fn type_if_ready(&self, tmux_name: &str, lines: &[String]) -> Result<(), MultiplexerError> {
        self.call("type_if_ready", tmux_name)?;
        self.edit(|state| state.type_lines("type_if_ready", tmux_name, lines));
        Ok(())
    }

    fn ensure_shell_window(
        &self,
        tmux_name: &str,
        _worktree: &Path,
        shell: &str,
    ) -> Result<String, MultiplexerError> {
        self.call("ensure_shell_window", &format!("{tmux_name} {shell}"))?;
        Ok("@9".to_string())
    }

    fn server_state(&self) -> Result<Option<ServerState>, MultiplexerError> {
        self.call("server_state", "")?;
        Ok(self.edit(|state| state.server.clone()))
    }

    fn server_config(
        &self,
        _key_bindings: &KeyBindings,
    ) -> Result<Option<ServerConfig>, MultiplexerError> {
        self.call("server_config", "")?;
        Ok(self.edit(|state| {
            state.server.as_ref().map(|_| {
                ServerConfig::new(state.config_current, state.server_key_bindings.as_ref())
            })
        }))
    }

    fn socket_lost(&self) -> bool {
        self.edit(|state| state.socket_lost)
    }

    fn restore_socket(&self) -> Result<bool, MultiplexerError> {
        self.call("restore_socket", "")?;
        Ok(self.edit(|state| std::mem::take(&mut state.socket_lost)))
    }

    fn apply_config(&self, key_bindings: &KeyBindings) -> Result<(), MultiplexerError> {
        let keys: Vec<String> = key_bindings.all().map(HotKey::to_string).collect();
        self.call("apply_config", &keys.join(" "))?;
        self.edit(|state| {
            state.config_current = true;
            state.server_key_bindings = Some(key_bindings.clone());
        });
        Ok(())
    }

    fn version(&self) -> Result<String, MultiplexerError> {
        self.call("version", "")?;
        Ok(self.edit(|state| state.client_version.clone()))
    }
}
