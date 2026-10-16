use thiserror::Error;

use crate::domain::Agent;

pub trait OpencodeDatabase: Send + Sync {
    fn read(&self, agent: &Agent, query: &str) -> Result<Option<String>, OpencodeDatabaseError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("could not read opencode's database directly: {0}")]
pub struct OpencodeDatabaseError(String);

impl OpencodeDatabaseError {
    pub fn new(detail: &str) -> Self {
        Self(detail.to_string())
    }
}
