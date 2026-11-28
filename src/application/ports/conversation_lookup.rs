use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::domain::Agent;

pub trait ConversationLookup: Send + Sync {
    fn find(&self, query: &ConversationQuery) -> Result<Option<String>, ConversationLookupError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationQuery {
    agent: Agent,
    argv: Vec<String>,
    worktree: PathBuf,
}

impl ConversationQuery {
    pub fn new(agent: &Agent, argv: Vec<String>, worktree: &Path) -> Self {
        Self {
            agent: agent.clone(),
            argv,
            worktree: worktree.to_path_buf(),
        }
    }

    pub fn agent(&self) -> &Agent {
        &self.agent
    }

    pub fn argv(&self) -> &[String] {
        &self.argv
    }

    pub fn worktree(&self) -> &Path {
        &self.worktree
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConversationLookupError {
    #[error("the conversation lookup took too long")]
    TimedOut,
    #[error("the conversation lookup did not answer")]
    Unanswered,
}
