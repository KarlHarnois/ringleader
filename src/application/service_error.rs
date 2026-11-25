use thiserror::Error;

use super::ports::{
    ConfigError, HostProbeError, LockError, MultiplexerError, RegistryError, RepoPageError,
    TerminalEmulatorError, WebBrowserError, WorkspaceError,
};
use crate::application::DeleteImpact;
use crate::application::FailureKind;
use crate::domain::InvalidName;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Git,
    Tmux,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ServiceError {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    LockHeld(String),
    #[error("{0}")]
    Unmeasurable(String),
    #[error("{message}")]
    GuardsChanged {
        message: String,
        impact: Box<DeleteImpact>,
    },
    #[error("{message}")]
    TimedOut { tool: Tool, message: String },
    #[error("{0}")]
    Failed(String),
}

impl ServiceError {
    pub fn failure_kind(&self) -> FailureKind {
        match self {
            ServiceError::Invalid(_) => FailureKind::Invalid,
            ServiceError::NotFound(_) => FailureKind::NotFound,
            ServiceError::Conflict(_) => FailureKind::Conflict,
            ServiceError::LockHeld(_) => FailureKind::LockHeld,
            ServiceError::Unmeasurable(_) => FailureKind::Unmeasurable,
            ServiceError::GuardsChanged { .. } => FailureKind::WouldLoseWork,
            ServiceError::TimedOut { .. } => FailureKind::TimedOut,
            ServiceError::Failed(_) => FailureKind::Failed,
        }
    }

    pub fn is_timeout_of(&self, expected: Tool) -> bool {
        matches!(self, ServiceError::TimedOut { tool, .. } if *tool == expected)
    }

    pub fn is_refusal(&self) -> bool {
        matches!(
            self,
            ServiceError::Conflict(_)
                | ServiceError::LockHeld(_)
                | ServiceError::GuardsChanged { .. }
        )
    }

    pub fn adding_context(self, context: &str) -> Self {
        let message = format!("{self}; {context}");
        match self {
            ServiceError::TimedOut { tool, .. } => ServiceError::TimedOut { tool, message },
            ServiceError::Invalid(_) => ServiceError::Invalid(message),
            ServiceError::NotFound(_) => ServiceError::NotFound(message),
            ServiceError::Conflict(_) => ServiceError::Conflict(message),
            ServiceError::LockHeld(_) => ServiceError::LockHeld(message),
            ServiceError::Unmeasurable(_) => ServiceError::Unmeasurable(message),
            ServiceError::GuardsChanged { impact, .. } => {
                ServiceError::GuardsChanged { message, impact }
            }
            ServiceError::Failed(_) => ServiceError::Failed(message),
        }
    }
}

impl From<WorkspaceError> for ServiceError {
    fn from(error: WorkspaceError) -> Self {
        let message = error.to_string();
        match error {
            WorkspaceError::TimedOut { .. } => ServiceError::TimedOut {
                tool: Tool::Git,
                message,
            },
            WorkspaceError::Failed { .. } => ServiceError::Failed(message),
            _ => ServiceError::Invalid(message),
        }
    }
}

impl From<MultiplexerError> for ServiceError {
    fn from(error: MultiplexerError) -> Self {
        let message = error.to_string();
        match error.is_timeout() {
            true => ServiceError::TimedOut {
                tool: Tool::Tmux,
                message,
            },
            false => ServiceError::Failed(message),
        }
    }
}

impl From<RegistryError> for ServiceError {
    fn from(error: RegistryError) -> Self {
        ServiceError::Failed(error.to_string())
    }
}

impl From<LockError> for ServiceError {
    fn from(error: LockError) -> Self {
        let message = error.to_string();
        match error {
            LockError::Held { .. } => ServiceError::LockHeld(message),
            LockError::Io { .. } => ServiceError::Failed(message),
        }
    }
}

impl From<ConfigError> for ServiceError {
    fn from(error: ConfigError) -> Self {
        ServiceError::Invalid(error.to_string())
    }
}

impl From<HostProbeError> for ServiceError {
    fn from(error: HostProbeError) -> Self {
        let message = error.to_string();
        match error {
            HostProbeError::RelativePath { .. } => ServiceError::Invalid(message),
            HostProbeError::OwnProcess { .. } | HostProbeError::BatchAboveCeiling { .. } => {
                ServiceError::Conflict(message)
            }
        }
    }
}

impl From<TerminalEmulatorError> for ServiceError {
    fn from(error: TerminalEmulatorError) -> Self {
        ServiceError::Failed(error.to_string())
    }
}

impl From<WebBrowserError> for ServiceError {
    fn from(error: WebBrowserError) -> Self {
        ServiceError::Failed(error.to_string())
    }
}

impl From<RepoPageError> for ServiceError {
    fn from(error: RepoPageError) -> Self {
        ServiceError::Failed(error.to_string())
    }
}

impl From<InvalidName> for ServiceError {
    fn from(error: InvalidName) -> Self {
        ServiceError::Invalid(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{FailureKind, LockError, ServiceError, Tool};

    #[test]
    fn each_service_error_names_its_failure_kind() {
        let cases = [
            (
                ServiceError::Invalid("bad".to_string()),
                FailureKind::Invalid,
            ),
            (
                ServiceError::NotFound("bad".to_string()),
                FailureKind::NotFound,
            ),
            (
                ServiceError::Conflict("bad".to_string()),
                FailureKind::Conflict,
            ),
            (
                ServiceError::LockHeld("bad".to_string()),
                FailureKind::LockHeld,
            ),
            (
                ServiceError::Unmeasurable("bad".to_string()),
                FailureKind::Unmeasurable,
            ),
            (
                ServiceError::GuardsChanged {
                    message: "bad".to_string(),
                    impact: Box::default(),
                },
                FailureKind::WouldLoseWork,
            ),
            (
                ServiceError::TimedOut {
                    tool: Tool::Git,
                    message: "bad".to_string(),
                },
                FailureKind::TimedOut,
            ),
            (ServiceError::Failed("bad".to_string()), FailureKind::Failed),
        ];

        for (error, kind) in cases {
            assert_eq!(error.failure_kind(), kind);
        }
    }

    #[test]
    fn adding_context_keeps_the_kind_and_appends_the_context() {
        let kinds = [
            ServiceError::Invalid("bad".to_string()),
            ServiceError::NotFound("bad".to_string()),
            ServiceError::Conflict("bad".to_string()),
            ServiceError::LockHeld("bad".to_string()),
            ServiceError::Unmeasurable("bad".to_string()),
            ServiceError::Failed("bad".to_string()),
            ServiceError::TimedOut {
                tool: Tool::Git,
                message: "bad".to_string(),
            },
        ];
        for error in kinds {
            let kind = std::mem::discriminant(&error);
            let extended = error.adding_context("while renaming");
            assert_eq!(std::mem::discriminant(&extended), kind);
            assert_eq!(extended.to_string(), "bad; while renaming");
        }
    }

    #[test]
    fn a_held_lock_becomes_a_lock_held_failure() {
        let error = ServiceError::from(LockError::Held {
            path: PathBuf::from("/state/ringleader/lock"),
        });

        assert_eq!(error.failure_kind(), FailureKind::LockHeld);
    }

    #[test]
    fn adding_context_keeps_a_guards_changed_impact() {
        let error = ServiceError::GuardsChanged {
            message: "bad".to_string(),
            impact: Box::default(),
        };

        let extended = error.adding_context("while renaming");

        assert!(matches!(extended, ServiceError::GuardsChanged { .. }));
        assert_eq!(extended.to_string(), "bad; while renaming");
    }

    #[test]
    fn adding_context_keeps_the_timed_out_tool() {
        let error = ServiceError::TimedOut {
            tool: Tool::Tmux,
            message: "slow".to_string(),
        };
        assert!(
            error
                .adding_context("while listing")
                .is_timeout_of(Tool::Tmux)
        );
    }
}
