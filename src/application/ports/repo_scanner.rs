use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::domain::{Repo, RepoScope};

pub trait RepoScanner: Send + Sync {
    fn resolve_scope(&self, launch_directory: &Path) -> Result<RepoScope, ScanError>;
    fn scan(&self, scope: &RepoScope, depth: usize) -> Result<ScanOutcome, ScanError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanOutcome {
    repos: Vec<Repo>,
    cut_short: bool,
}

impl ScanOutcome {
    pub fn new(repos: Vec<Repo>, cut_short: bool) -> Self {
        Self { repos, cut_short }
    }

    pub fn repos(&self) -> &[Repo] {
        &self.repos
    }

    pub fn into_repos(self) -> Vec<Repo> {
        self.repos
    }

    pub fn cut_short(&self) -> bool {
        self.cut_short
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ScanError {
    #[error("cannot list {path}: {detail}")]
    Unreadable { path: PathBuf, detail: String },
}
