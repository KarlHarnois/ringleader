use std::path::PathBuf;

use thiserror::Error;

pub trait Lock {
    fn acquire_lifetime(&mut self) -> Result<(), LockError>;
    fn acquire_for_write(&mut self) -> Result<(), LockError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LockError {
    #[error("another instance is running, it holds {path}")]
    Held { path: PathBuf },
    #[error("cannot lock {path}: {detail}")]
    Io { path: PathBuf, detail: String },
}
