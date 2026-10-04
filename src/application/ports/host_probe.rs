use std::fmt;
use std::path::{Path, PathBuf};

use thiserror::Error;

pub trait HostProbe: Send + Sync {
    fn resolve_command(&self, command: &str) -> Option<PathBuf>;
    fn is_writable(&self, path: &Path) -> bool;
    fn path_exists(&self, path: &Path) -> bool;
    fn is_file(&self, path: &Path) -> bool;
    fn canonicalize_lenient(&self, path: &Path) -> Result<PathBuf, HostProbeError>;
    fn processes_in(&self, path: &Path, excluded_sessions: &[u32]) -> Vec<ProcessInfo>;
    fn session_processes(&self, sessions: &[u32]) -> Vec<ProcessInfo>;
    fn terminate(&self, pids: &[u32]) -> Vec<ProcessInfo>;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProcessInfo {
    pid: u32,
    command: String,
}

impl ProcessInfo {
    pub fn new(pid: u32, command: &str) -> Self {
        Self {
            pid,
            command: command.to_string(),
        }
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    #[cfg(test)]
    pub fn command(&self) -> &str {
        &self.command
    }
}

impl fmt::Display for ProcessInfo {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} ({})", self.pid, self.command)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HostProbeError {
    #[error("{path} is not an absolute path")]
    RelativePath { path: PathBuf },
}
