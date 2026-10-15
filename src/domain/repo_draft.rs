use std::path::{Path, PathBuf};

use crate::domain::BaseBranch;
use crate::domain::Repo;
use crate::domain::RepoName;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoDraft {
    name: Option<String>,
    path: PathBuf,
    base_branch: Option<String>,
}

impl RepoDraft {
    pub fn new(path: &Path) -> Self {
        Self {
            name: None,
            path: path.to_path_buf(),
            base_branch: None,
        }
    }

    pub fn named(self, name: &str) -> Self {
        Self {
            name: Some(name.to_string()),
            ..self
        }
    }

    pub fn based_on(self, base_branch: &str) -> Self {
        Self {
            base_branch: Some(base_branch.to_string()),
            ..self
        }
    }

    fn repo_name(&self) -> Option<RepoName> {
        match &self.name {
            Some(name) => RepoName::parse(name).ok(),
            None => RepoName::of_directory(&self.path).ok(),
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn shown_name(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| self.path.display().to_string())
    }

    pub(super) fn resolved_name(&self) -> Option<RepoName> {
        self.repo_name()
    }

    pub(super) fn repo(&self) -> Option<Repo> {
        let base_branch = self.base_branch.as_deref().and_then(BaseBranch::new);
        Some(Repo::explicit(self.repo_name()?, &self.path, base_branch))
    }
}
