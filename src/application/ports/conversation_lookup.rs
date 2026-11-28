use std::path::Path;

use thiserror::Error;

use crate::domain::Agent;

pub trait ConversationLookup: Send + Sync {
    fn find(
        &self,
        agent: &Agent,
        worktree: &Path,
    ) -> Result<Option<String>, ConversationLookupError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConversationLookupError {
    #[error("the conversation lookup took too long")]
    TimedOut,
    #[error("the conversation lookup did not answer")]
    Unanswered,
}
