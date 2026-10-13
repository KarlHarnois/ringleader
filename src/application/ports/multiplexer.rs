use std::path::{Path, PathBuf};

use thiserror::Error;

use super::agent_launch::AgentLaunch;
use crate::domain::{HotKey, SessionMetadata, SessionObservation, SessionSize};

pub trait Multiplexer: Send + Sync {
    fn socket(&self) -> PathBuf;
    fn create_session(&self, spec: &SessionSpec) -> Result<PathBuf, MultiplexerError>;
    fn kill_session(&self, tmux_name: &str) -> Result<(), MultiplexerError>;
    fn pane_pids(&self, tmux_name: &str) -> Result<Vec<u32>, MultiplexerError>;
    fn caller_session(&self) -> Result<Option<String>, MultiplexerError>;
    fn caller_in_own_pane(&self) -> Result<bool, MultiplexerError>;
    fn caller_in_other_tmux(&self) -> bool;
    fn observe_all(&self) -> Result<Vec<SessionObservation>, MultiplexerError>;
    fn attach(&self, target: &AttachTarget) -> Result<(), MultiplexerError>;
    fn capture_pane(&self, tmux_name: &str) -> Result<String, MultiplexerError>;
    fn rename_session(
        &self,
        old: &str,
        new: &str,
        metadata: &SessionMetadata,
    ) -> Result<(), MultiplexerError>;
    fn respawn_agent(&self, tmux_name: &str, launch: &AgentLaunch) -> Result<(), MultiplexerError>;
    fn type_when_ready(&self, tmux_name: &str, lines: &[String]) -> Result<(), MultiplexerError>;
    fn type_if_ready(&self, tmux_name: &str, lines: &[String]) -> Result<(), MultiplexerError>;
    fn ensure_shell_window(
        &self,
        tmux_name: &str,
        worktree: &Path,
        shell: &str,
    ) -> Result<String, MultiplexerError>;
    fn server_state(&self) -> Result<Option<ServerState>, MultiplexerError>;
    fn server_config(&self, return_key: &HotKey) -> Result<Option<ServerConfig>, MultiplexerError>;
    fn socket_lost(&self) -> bool;
    fn restore_socket(&self) -> Result<bool, MultiplexerError>;
    fn apply_config(&self, return_key: &HotKey) -> Result<(), MultiplexerError>;
    fn version(&self) -> Result<String, MultiplexerError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSpec {
    tmux_name: String,
    metadata: SessionMetadata,
    launch: AgentLaunch,
    size: SessionSize,
    removed_env_names: Vec<String>,
    return_key: HotKey,
}

impl SessionSpec {
    pub fn new(tmux_name: &str, metadata: SessionMetadata, launch: AgentLaunch) -> Self {
        Self {
            tmux_name: tmux_name.to_string(),
            metadata,
            launch,
            size: SessionSize::default(),
            removed_env_names: Vec::new(),
            return_key: HotKey::default(),
        }
    }

    pub fn sized(self, size: SessionSize) -> Self {
        Self { size, ..self }
    }

    pub fn removing_env_names(self, names: &[String]) -> Self {
        Self {
            removed_env_names: names.to_vec(),
            ..self
        }
    }

    pub fn with_return_key(self, return_key: &HotKey) -> Self {
        Self {
            return_key: return_key.clone(),
            ..self
        }
    }

    pub fn tmux_name(&self) -> &str {
        &self.tmux_name
    }

    pub fn metadata(&self) -> &SessionMetadata {
        &self.metadata
    }

    pub fn launch(&self) -> &AgentLaunch {
        &self.launch
    }

    pub fn worktree(&self) -> &Path {
        self.metadata.worktree_path()
    }

    pub fn size(&self) -> SessionSize {
        self.size
    }

    pub fn removed_env_names(&self) -> &[String] {
        &self.removed_env_names
    }

    pub fn return_key(&self) -> &HotKey {
        &self.return_key
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerState {
    version: String,
    sessions: usize,
}

impl ServerState {
    pub fn new(version: &str, sessions: usize) -> Self {
        Self {
            version: version.to_string(),
            sessions,
        }
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn sessions(&self) -> usize {
        self.sessions
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    current: bool,
    return_key: Option<HotKey>,
}

impl ServerConfig {
    pub fn new(current: bool, return_key: Option<&HotKey>) -> Self {
        Self {
            current,
            return_key: return_key.cloned(),
        }
    }

    pub fn is_current(&self) -> bool {
        self.current
    }

    pub fn return_key(&self) -> Option<&HotKey> {
        self.return_key.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachTarget {
    Agent(String),
    Shell(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MultiplexerError {
    #[error("tmux {subcommand} failed: {detail}")]
    Failed { subcommand: String, detail: String },
    #[error("tmux {subcommand} timed out after {seconds} seconds")]
    TimedOut { subcommand: String, seconds: u64 },
    #[error(
        "the tmux server at {socket} binds no key to detach-client, so its sessions could not be left; check {config}"
    )]
    NoReturnBinding { socket: String, config: String },
    #[error(
        "Ringleader's tmux server runs without its socket {socket}, and recreating it failed: {detail}"
    )]
    SocketNotRestored { socket: String, detail: String },
    #[error("cannot prepare the tmux server files: {detail}")]
    ServerFiles { detail: String },
    #[error("cannot find the agent pane of tmux session {tmux_name}")]
    NoAgentPane { tmux_name: String },
    #[error("the agent in tmux session {tmux_name} is not at its prompt")]
    AgentNotReady { tmux_name: String },
    #[error("cannot write the agent handoff file: {detail}")]
    Handoff { detail: String },
}

impl MultiplexerError {
    pub fn is_timeout(&self) -> bool {
        matches!(self, MultiplexerError::TimedOut { .. })
    }
}
