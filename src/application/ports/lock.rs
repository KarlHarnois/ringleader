use std::path::PathBuf;

use thiserror::Error;

use crate::application::FailureKind;

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

impl LockError {
    pub fn failure_kind(&self) -> FailureKind {
        match self {
            LockError::Held { .. } => FailureKind::LockHeld,
            LockError::Io { .. } => FailureKind::Failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_lock_error_names_its_failure_kind() {
        let cases = [
            (
                LockError::Held {
                    path: PathBuf::from("/state/ringleader/lock"),
                },
                FailureKind::LockHeld,
            ),
            (
                LockError::Io {
                    path: PathBuf::from("/state/ringleader/lock"),
                    detail: "permission denied".to_string(),
                },
                FailureKind::Failed,
            ),
        ];

        for (error, kind) in cases {
            assert_eq!(error.failure_kind(), kind);
        }
    }
}
