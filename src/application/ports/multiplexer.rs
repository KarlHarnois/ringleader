use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::domain::{Agent, SessionMetadata, SessionObservation, SessionSize};

pub trait Multiplexer: Send + Sync {
    fn create_session(&self, spec: &SessionSpec) -> Result<PathBuf, MultiplexerError>;
    fn kill_session(&self, tmux_name: &str) -> Result<(), MultiplexerError>;
    fn pane_pids(&self, tmux_name: &str) -> Result<Vec<u32>, MultiplexerError>;
    fn has_session_on(
        &self,
        socket: &Path,
        tmux_name: &str,
        worktree: &Path,
    ) -> Result<bool, MultiplexerError>;
    fn caller_session(&self) -> Result<Option<String>, MultiplexerError>;
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
    fn ring_bell(&self) -> Result<(), MultiplexerError>;
    fn server_version(&self) -> Result<Option<String>, MultiplexerError>;
    fn server_exit_unattached(&self) -> Result<Option<bool>, MultiplexerError>;
    fn version(&self) -> Result<String, MultiplexerError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSpec {
    tmux_name: String,
    metadata: SessionMetadata,
    launch: AgentLaunch,
    size: SessionSize,
    removed_env_names: Vec<String>,
}

impl SessionSpec {
    pub fn new(tmux_name: &str, metadata: SessionMetadata, launch: AgentLaunch) -> Self {
        Self {
            tmux_name: tmux_name.to_string(),
            metadata,
            launch,
            size: SessionSize::default(),
            removed_env_names: Vec::new(),
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLaunch {
    argv: Vec<String>,
    env_names: Vec<String>,
    env_assignments: Vec<(String, String)>,
    worktree: PathBuf,
}

impl AgentLaunch {
    pub fn new(argv: Vec<String>, env_names: &[String], worktree: &Path) -> Self {
        Self {
            argv,
            env_names: env_names.to_vec(),
            env_assignments: Vec::new(),
            worktree: worktree.to_path_buf(),
        }
    }

    pub fn for_agent(agent: &Agent, argv: Vec<String>, worktree: &Path) -> Self {
        Self::new(argv, agent.env_names(), worktree).with_env_assignments(agent.env_assignments())
    }

    pub fn with_env_assignments(self, assignments: &[(String, String)]) -> Self {
        Self {
            env_assignments: assignments.to_vec(),
            ..self
        }
    }

    pub fn argv(&self) -> &[String] {
        &self.argv
    }

    pub fn env_names(&self) -> &[String] {
        &self.env_names
    }

    pub fn env_assignments(&self) -> &[(String, String)] {
        &self.env_assignments
    }

    pub fn worktree(&self) -> &Path {
        &self.worktree
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
        "the tmux server has exit-unattached on, which ends every detached session; turn it off to create sessions"
    )]
    ExitUnattached,
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
