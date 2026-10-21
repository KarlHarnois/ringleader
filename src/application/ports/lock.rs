use std::path::PathBuf;

use thiserror::Error;

pub trait Lock {
    fn try_acquire(&mut self) -> Result<(), LockError>;
    fn acquire(&mut self, stop: &dyn Fn() -> bool) -> Result<(), LockError>;
    fn release(&mut self);
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LockError {
    #[error("another Ringleader holds {path}")]
    Held { path: PathBuf },
    #[error("cannot lock {path}: {detail}")]
    Io { path: PathBuf, detail: String },
}
