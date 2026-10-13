use std::fmt;
use std::path::{Path, PathBuf};

use thiserror::Error;

pub trait HostProbe: Send + Sync {
    fn resolve_command(&self, command: &str) -> Option<PathBuf>;
    fn is_writable(&self, path: &Path) -> bool;
    fn path_exists(&self, path: &Path) -> bool;
    fn is_file(&self, path: &Path) -> bool;
    fn canonicalize_lenient(&self, path: &Path) -> Result<PathBuf, HostProbeError>;
    fn canonical_or_given(&self, path: &Path) -> PathBuf {
        self.canonicalize_lenient(path)
            .unwrap_or_else(|_| path.to_path_buf())
    }
    fn directory_access(&self, path: &Path) -> Option<DirectoryAccess>;
    fn processes_in(&self, path: &Path, excluded_sessions: &[u32]) -> Vec<ProcessInfo>;
    fn processes_under_server(&self, path: &Path, socket: &Path) -> Vec<ProcessInfo>;
    fn tmux_server_serves(&self, socket: &Path) -> bool;
    fn session_processes(&self, sessions: &[u32]) -> Vec<ProcessInfo>;
    fn terminate(&self, pids: &[u32]) -> Vec<ProcessInfo>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectoryAccess {
    mode: u32,
    owned_by_user: bool,
}

impl DirectoryAccess {
    pub fn new(mode: u32, owned_by_user: bool) -> Self {
        Self {
            mode,
            owned_by_user,
        }
    }

    pub fn mode(&self) -> u32 {
        self.mode
    }

    pub fn owned_by_user(&self) -> bool {
        self.owned_by_user
    }
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

    pub fn listed(processes: &[ProcessInfo]) -> String {
        let shown: Vec<String> = processes.iter().map(ToString::to_string).collect();
        shown.join(", ")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::FakeHostProbe;

    #[test]
    fn a_path_with_a_canonical_form_is_given_as_that_form() {
        let host = FakeHostProbe::default()
            .with_canonical(Path::new("/link/example"), Path::new("/work/example"));

        assert_eq!(
            host.canonical_or_given(Path::new("/link/example")),
            PathBuf::from("/work/example")
        );
    }

    #[test]
    fn a_path_that_cannot_be_canonicalized_is_given_back() {
        let host = FakeHostProbe::default();

        assert_eq!(
            host.canonical_or_given(Path::new("relative/example")),
            PathBuf::from("relative/example")
        );
    }
}
