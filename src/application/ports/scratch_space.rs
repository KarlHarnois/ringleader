use std::path::{Path, PathBuf};

use thiserror::Error;

pub trait ScratchSpace: Send + Sync {
    fn directory(&self, worktree: &Path) -> PathBuf;
    fn create(&self, worktree: &Path) -> Result<(), ScratchSpaceError>;
    fn remove(&self, worktree: &Path) -> Result<(), ScratchSpaceError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ScratchSpaceError {
    #[error("cannot {operation} the scratch directory {path}: {detail}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        detail: String,
    },
    #[error("{path} is not a private directory of this user")]
    NotPrivate { path: PathBuf },
}
