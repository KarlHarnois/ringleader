use std::path::{Path, PathBuf};

use thiserror::Error;

pub trait LeftoverBindings: Send + Sync {
    fn path(&self) -> PathBuf;
    fn read(&self) -> Result<Vec<LeftoverBinding>, LeftoverBindingsError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeftoverBinding {
    socket: PathBuf,
    saved_line: Option<String>,
}

impl LeftoverBinding {
    pub fn new(socket: &Path, saved_line: Option<&str>) -> Self {
        Self {
            socket: socket.to_path_buf(),
            saved_line: saved_line.map(str::to_string),
        }
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub fn saved_line(&self) -> Option<&str> {
        self.saved_line.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LeftoverBindingsError {
    #[error("cannot read {path}: {detail}")]
    Unreadable { path: PathBuf, detail: String },
    #[error("{path} is not valid JSON: {detail}")]
    Corrupt { path: PathBuf, detail: String },
}
