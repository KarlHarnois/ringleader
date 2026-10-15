use std::path::{Path, PathBuf};

use crate::domain::BaseBranch;
use crate::domain::RepoName;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionMetadata {
    repo_name: RepoName,
    repo_path: PathBuf,
    branch: String,
    base_commit: String,
    requested_base_branch: Option<BaseBranch>,
    worktree_path: PathBuf,
    agent: String,
}

impl SessionMetadata {
    pub fn new(repo: SessionRepo, worktree: SessionWorktree, agent: &str) -> Self {
        Self {
            repo_name: repo.name,
            repo_path: repo.path,
            branch: worktree.branch,
            base_commit: worktree.base_commit,
            requested_base_branch: worktree.requested_base_branch,
            worktree_path: worktree.path,
            agent: agent.to_string(),
        }
    }

    pub fn repo_name(&self) -> &RepoName {
        &self.repo_name
    }

    pub fn repo_path(&self) -> &Path {
        &self.repo_path
    }

    pub fn branch(&self) -> &str {
        &self.branch
    }

    pub fn base_commit(&self) -> &str {
        &self.base_commit
    }

    pub fn requested_base_branch(&self) -> Option<&BaseBranch> {
        self.requested_base_branch.as_ref()
    }

    pub fn worktree_path(&self) -> &Path {
        &self.worktree_path
    }

    pub fn agent(&self) -> &str {
        &self.agent
    }

    pub fn with_branch(&self, branch: &str) -> Self {
        Self {
            branch: branch.to_string(),
            ..self.clone()
        }
    }

    pub fn has_absolute_paths(&self) -> bool {
        self.repo_path.is_absolute() && self.worktree_path.is_absolute()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRepo {
    name: RepoName,
    path: PathBuf,
}

impl SessionRepo {
    pub fn new(name: RepoName, path: &Path) -> Self {
        Self {
            name,
            path: path.to_path_buf(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionWorktree {
    path: PathBuf,
    branch: String,
    base_commit: String,
    requested_base_branch: Option<BaseBranch>,
}

impl SessionWorktree {
    pub fn new(path: &Path, branch: &str, base_commit: &str) -> Self {
        Self {
            path: path.to_path_buf(),
            branch: branch.to_string(),
            base_commit: base_commit.to_string(),
            requested_base_branch: None,
        }
    }

    pub fn based_on(self, requested_base_branch: Option<BaseBranch>) -> Self {
        Self {
            requested_base_branch,
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MetadataFixture;

    impl MetadataFixture {
        fn with_paths(repo_path: &str, worktree_path: &str) -> SessionMetadata {
            SessionMetadata::new(
                SessionRepo::new(
                    RepoName::parse("example-project").unwrap(),
                    Path::new(repo_path),
                ),
                SessionWorktree::new(Path::new(worktree_path), "ringleader/fix-auth", "abc123"),
                "claude",
            )
        }
    }

    #[test]
    fn absolute_paths_are_accepted() {
        let metadata = MetadataFixture::with_paths("/repos/example", "/worktrees/fix-auth");

        assert!(metadata.has_absolute_paths());
    }

    #[test]
    fn a_relative_worktree_path_is_rejected() {
        let metadata = MetadataFixture::with_paths("/repos/example", "worktrees/fix-auth");

        assert!(!metadata.has_absolute_paths());
    }

    #[test]
    fn with_branch_replaces_only_the_branch() {
        let metadata = MetadataFixture::with_paths("/repos/example", "/worktrees/fix-auth");

        let renamed = metadata.with_branch("ringleader/fix-login");

        assert_eq!(renamed.branch(), "ringleader/fix-login");
        assert_eq!(renamed.worktree_path(), metadata.worktree_path());
    }
}
