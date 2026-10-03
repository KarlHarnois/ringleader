use std::path::{Path, PathBuf};

use super::base_branch::BaseBranch;
use super::repo_name::RepoName;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    name: RepoName,
    path: PathBuf,
    base_branch: Option<BaseBranch>,
    explicit: bool,
}

impl Repo {
    pub fn discovered(name: RepoName, path: &Path) -> Self {
        Self {
            name,
            path: path.to_path_buf(),
            base_branch: None,
            explicit: false,
        }
    }

    pub fn explicit(name: RepoName, path: &Path, base_branch: Option<BaseBranch>) -> Self {
        Self {
            name,
            path: path.to_path_buf(),
            base_branch,
            explicit: true,
        }
    }

    pub fn name(&self) -> &RepoName {
        &self.name
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn base_branch(&self) -> Option<&BaseBranch> {
        self.base_branch.as_ref()
    }

    pub fn is_explicit(&self) -> bool {
        self.explicit
    }

    pub fn at_path(&self, path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            ..self.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoScope {
    root: PathBuf,
    enclosing_repo: Option<PathBuf>,
    unresolved_git_file: Option<PathBuf>,
}

impl RepoScope {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            enclosing_repo: None,
            unresolved_git_file: None,
        }
    }

    pub fn inside_repo(self, top_level: &Path) -> Self {
        Self {
            enclosing_repo: Some(top_level.to_path_buf()),
            ..self
        }
    }

    pub fn stopped_at_git_file(self, directory: &Path) -> Self {
        Self {
            unresolved_git_file: Some(directory.to_path_buf()),
            ..self
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn enclosing_repo(&self) -> Option<&Path> {
        self.enclosing_repo.as_deref()
    }

    pub fn unresolved_git_file(&self) -> Option<&Path> {
        self.unresolved_git_file.as_deref()
    }
}
