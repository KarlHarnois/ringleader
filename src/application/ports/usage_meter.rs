use thiserror::Error;

use crate::domain::{Agent, AgentUsage};

pub trait UsageMeter: Send + Sync {
    fn measure(&self, agent: &Agent) -> Result<Option<AgentUsage>, UsageError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum UsageError {
    #[error("the agent took too long to report its usage")]
    TimedOut,
    #[error("the agent did not report its usage")]
    Unanswered,
    #[error("the agent's usage report could not be read")]
    Unreadable,
}
