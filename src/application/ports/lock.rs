use std::path::PathBuf;
use std::time::Duration;

use thiserror::Error;

use crate::application::FailureKind;

pub trait Lock {
    fn try_acquire(&mut self) -> Result<(), LockError>;
    fn acquire(&mut self, patience: LockPatience, stop: &dyn Fn() -> bool)
    -> Result<(), LockError>;
    fn release(&mut self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockPatience {
    limit: Duration,
}

impl LockPatience {
    pub const DEFAULT: Self = Self {
        limit: Duration::from_secs(60),
    };

    pub fn of(limit: Duration) -> Self {
        Self { limit }
    }

    pub fn limit(&self) -> Duration {
        self.limit
    }

    pub fn allows_a_wait(&self) -> bool {
        !self.limit.is_zero()
    }
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
    fn a_zero_patience_allows_no_wait() {
        assert!(!LockPatience::of(Duration::from_secs(0)).allows_a_wait());
    }

    #[test]
    fn the_default_patience_allows_a_wait_for_sixty_seconds() {
        assert_eq!(LockPatience::DEFAULT.limit(), Duration::from_secs(60));
        assert!(LockPatience::DEFAULT.allows_a_wait());
    }

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
