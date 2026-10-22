use std::path::Path;

use thiserror::Error;

pub trait RepoPages: Send + Sync {
    fn main_page(&self, repo: &Path) -> Result<Option<String>, RepoPageError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RepoPageError {
    #[error("looking up the GitHub page timed out after {seconds} seconds")]
    TimedOut { seconds: u64 },
    #[error("cannot start the GitHub page lookup")]
    NotStarted,
}
