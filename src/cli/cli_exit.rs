use crate::application::ServiceError;
use crate::application::ports::LockError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliExit {
    Code(u8),
    Interrupted { signal: i32, code: u8 },
}

impl CliExit {
    pub const FAILURE: u8 = 1;
    pub const NOT_FOUND: u8 = 2;
    pub const CONFLICT: u8 = 3;

    pub fn error_code(error: &ServiceError) -> u8 {
        match error {
            ServiceError::NotFound(_) => Self::NOT_FOUND,
            ServiceError::Conflict(_)
            | ServiceError::Unmeasurable(_)
            | ServiceError::GuardsChanged { .. } => Self::CONFLICT,
            _ => Self::FAILURE,
        }
    }

    pub fn lock_error_code(error: &LockError) -> u8 {
        match error {
            LockError::Held { .. } => Self::CONFLICT,
            LockError::Io { .. } => Self::FAILURE,
        }
    }

    pub fn code(&self) -> u8 {
        match self {
            CliExit::Code(code) | CliExit::Interrupted { code, .. } => *code,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    struct ErrorFixture;

    impl ErrorFixture {
        fn message() -> String {
            "the operation failed".to_string()
        }

        fn lock_path() -> PathBuf {
            PathBuf::from("/state/ringleader/lock")
        }
    }

    #[test]
    fn a_missing_session_exits_two() {
        let error = ServiceError::NotFound(ErrorFixture::message());

        assert_eq!(CliExit::error_code(&error), CliExit::NOT_FOUND);
    }

    #[test]
    fn a_conflict_exits_three() {
        let error = ServiceError::Conflict(ErrorFixture::message());

        assert_eq!(CliExit::error_code(&error), CliExit::CONFLICT);
    }

    #[test]
    fn a_worktree_that_cannot_be_measured_exits_three() {
        let error = ServiceError::Unmeasurable(ErrorFixture::message());

        assert_eq!(CliExit::error_code(&error), CliExit::CONFLICT);
    }

    #[test]
    fn an_invalid_request_exits_one() {
        let error = ServiceError::Invalid(ErrorFixture::message());

        assert_eq!(CliExit::error_code(&error), CliExit::FAILURE);
    }

    #[test]
    fn a_failure_exits_one() {
        let error = ServiceError::Failed(ErrorFixture::message());

        assert_eq!(CliExit::error_code(&error), CliExit::FAILURE);
    }

    #[test]
    fn a_lock_held_elsewhere_exits_three() {
        let error = LockError::Held {
            path: ErrorFixture::lock_path(),
        };

        assert_eq!(CliExit::lock_error_code(&error), CliExit::CONFLICT);
    }

    #[test]
    fn a_lock_that_cannot_be_taken_exits_one() {
        let error = LockError::Io {
            path: ErrorFixture::lock_path(),
            detail: "permission denied".to_string(),
        };

        assert_eq!(CliExit::lock_error_code(&error), CliExit::FAILURE);
    }
}
