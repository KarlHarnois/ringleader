use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::application::ports::{
    AgentLaunch, AttachTarget, Multiplexer, MultiplexerError, SessionSpec,
};
use crate::domain::{AgentPane, SessionMetadata, SessionObservation, TmuxAddress};

pub struct FakeMultiplexer {
    state: Mutex<MultiplexerState>,
}

struct MultiplexerState {
    socket: PathBuf,
    live: Vec<SessionObservation>,
    specs: Vec<SessionSpec>,
    launches: Vec<AgentLaunch>,
    pane_pids: HashMap<String, Vec<u32>>,
    sessions_on_other_sockets: Vec<(PathBuf, String, PathBuf)>,
    caller: Option<String>,
    captures: HashMap<String, String>,
    failures: HashMap<&'static str, MultiplexerError>,
    failed_create_still_creates: bool,
    attached: Vec<AttachTarget>,
    bells: usize,
    calls: Vec<String>,
}

impl FakeMultiplexer {
    pub const SOCKET: &'static str = "/tmp/tmux-1000/default";
    pub const STATE_CHANGING: [&'static str; 5] = [
        "create_session",
        "kill_session",
        "rename_session",
        "respawn_agent",
        "ensure_shell_window",
    ];

    pub fn new() -> Self {
        Self {
            state: Mutex::new(MultiplexerState {
                socket: PathBuf::from(Self::SOCKET),
                live: Vec::new(),
                specs: Vec::new(),
                launches: Vec::new(),
                pane_pids: HashMap::new(),
                sessions_on_other_sockets: Vec::new(),
                caller: None,
                captures: HashMap::new(),
                failures: HashMap::new(),
                failed_create_still_creates: false,
                attached: Vec::new(),
                bells: 0,
                calls: Vec::new(),
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

    pub fn with_session_on_socket(self, socket: &Path, tmux_name: &str, worktree: &Path) -> Self {
        let entry = (
            socket.to_path_buf(),
            tmux_name.to_string(),
            worktree.to_path_buf(),
        );
        self.edit(|state| state.sessions_on_other_sockets.push(entry));
        self
    }

    pub fn with_caller(self, tmux_name: &str) -> Self {
        self.edit(|state| state.caller = Some(tmux_name.to_string()));
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

    pub fn creating_despite_failure(self) -> Self {
        self.edit(|state| state.failed_create_still_creates = true);
        self
    }

    pub fn fail(&self, operation: &'static str, error: MultiplexerError) {
        self.edit(|state| state.failures.insert(operation, error));
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

    pub fn bells(&self) -> usize {
        self.edit(|state| state.bells)
    }

    fn edit<T>(&self, change: impl FnOnce(&mut MultiplexerState) -> T) -> T {
        change(&mut self.state.lock().unwrap())
    }

    fn call(&self, operation: &'static str, detail: &str) -> Result<(), MultiplexerError> {
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

    fn find_live(&self, tmux_name: &str) -> Option<&SessionObservation> {
        self.live.iter().find(|live| live.tmux_name() == tmux_name)
    }

    fn other_socket_holds(&self, socket: &Path, tmux_name: &str, worktree: &Path) -> bool {
        self.sessions_on_other_sockets
            .iter()
            .any(|(on, name, claim)| on == socket && (name == tmux_name || claim == worktree))
    }
}

impl Multiplexer for FakeMultiplexer {
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

    fn has_session_on(
        &self,
        socket: &Path,
        tmux_name: &str,
        worktree: &Path,
    ) -> Result<bool, MultiplexerError> {
        self.call("has_session_on", &socket.display().to_string())?;
        Ok(self.edit(|state| state.other_socket_holds(socket, tmux_name, worktree)))
    }

    fn caller_session(&self) -> Result<Option<String>, MultiplexerError> {
        Ok(self.edit(|state| state.caller.clone()))
    }

    fn observe_all(&self) -> Result<Vec<SessionObservation>, MultiplexerError> {
        self.call("observe_all", "")?;
        Ok(self.edit(|state| state.live.clone()))
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

    fn ensure_shell_window(
        &self,
        tmux_name: &str,
        _worktree: &Path,
        shell: &str,
    ) -> Result<String, MultiplexerError> {
        self.call("ensure_shell_window", &format!("{tmux_name} {shell}"))?;
        Ok("@9".to_string())
    }

    fn ring_bell(&self) -> Result<(), MultiplexerError> {
        self.call("ring_bell", "")?;
        self.edit(|state| state.bells += 1);
        Ok(())
    }

    fn server_version(&self) -> Result<Option<String>, MultiplexerError> {
        self.call("server_version", "")?;
        Ok(Some("3.6a".to_string()))
    }

    fn server_exit_unattached(&self) -> Result<Option<bool>, MultiplexerError> {
        self.call("server_exit_unattached", "")?;
        Ok(Some(false))
    }

    fn version(&self) -> Result<String, MultiplexerError> {
        self.call("version", "")?;
        Ok("3.6a".to_string())
    }
}
