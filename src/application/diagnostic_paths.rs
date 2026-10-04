use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticPaths {
    tmux_program: String,
    git_program: String,
    state_directory: PathBuf,
}

impl DiagnosticPaths {
    pub fn new(tmux_program: &str, git_program: &str, state_directory: &Path) -> Self {
        Self {
            tmux_program: tmux_program.to_string(),
            git_program: git_program.to_string(),
            state_directory: state_directory.to_path_buf(),
        }
    }

    pub fn tmux_program(&self) -> &str {
        &self.tmux_program
    }

    pub fn git_program(&self) -> &str {
        &self.git_program
    }

    pub fn state_directory(&self) -> &Path {
        &self.state_directory
    }
}

impl Default for DiagnosticPaths {
    fn default() -> Self {
        Self::new("tmux", "git", Path::new("/"))
    }
}
