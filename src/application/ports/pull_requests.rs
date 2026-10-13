use std::collections::HashMap;
use std::path::Path;

use thiserror::Error;

use crate::domain::PullRequest;

pub trait PullRequests: Send + Sync {
    fn lookup(
        &self,
        repo: &Path,
        branches: &[String],
    ) -> Result<HashMap<String, Option<PullRequest>>, PullRequestError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PullRequestError {
    #[error("gh {subcommand} timed out after {seconds} seconds")]
    TimedOut { subcommand: String, seconds: u64 },
    #[error("gh {subcommand} failed with {status}: {stderr}")]
    Failed {
        subcommand: String,
        status: String,
        stderr: String,
    },
}
