use thiserror::Error;

use super::delete_impact::DeleteImpact;
use super::ports::{
    ConfigError, HostProbeError, MultiplexerError, PullRequestError, RegistryError, WorkspaceError,
};
use crate::domain::InvalidName;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Git,
    Tmux,
    Gh,
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
    pub const INVALID_EXIT: u8 = 1;
    pub const NOT_FOUND_EXIT: u8 = 2;
    pub const CONFLICT_EXIT: u8 = 3;

    pub fn exit_code(&self) -> u8 {
        match self {
            ServiceError::NotFound(_) => Self::NOT_FOUND_EXIT,
            ServiceError::Conflict(_)
            | ServiceError::Unmeasurable(_)
            | ServiceError::GuardsChanged { .. } => Self::CONFLICT_EXIT,
            _ => Self::INVALID_EXIT,
        }
    }

    pub fn is_timeout_of(&self, expected: Tool) -> bool {
        matches!(self, ServiceError::TimedOut { tool, .. } if *tool == expected)
    }

    pub fn adding_context(self, context: &str) -> Self {
        let message = format!("{self}; {context}");
        match self {
            ServiceError::TimedOut { tool, .. } => ServiceError::TimedOut { tool, message },
            ServiceError::Invalid(_) => ServiceError::Invalid(message),
            ServiceError::NotFound(_) => ServiceError::NotFound(message),
            ServiceError::Conflict(_) => ServiceError::Conflict(message),
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

impl From<PullRequestError> for ServiceError {
    fn from(error: PullRequestError) -> Self {
        let message = error.to_string();
        match error {
            PullRequestError::TimedOut { .. } => ServiceError::TimedOut {
                tool: Tool::Gh,
                message,
            },
            PullRequestError::Failed { .. } => ServiceError::Failed(message),
        }
    }
}

impl From<RegistryError> for ServiceError {
    fn from(error: RegistryError) -> Self {
        ServiceError::Failed(error.to_string())
    }
}

impl From<ConfigError> for ServiceError {
    fn from(error: ConfigError) -> Self {
        ServiceError::Invalid(error.to_string())
    }
}

impl From<HostProbeError> for ServiceError {
    fn from(error: HostProbeError) -> Self {
        ServiceError::Invalid(error.to_string())
    }
}

impl From<InvalidName> for ServiceError {
    fn from(error: InvalidName) -> Self {
        ServiceError::Invalid(error.to_string())
    }
}
