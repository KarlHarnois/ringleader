use std::path::{Path, PathBuf};

use thiserror::Error;

use super::diff_base::DiffBase;
use super::dirty_state::DirtyState;
use super::unmerged_commits::UnmergedCommits;
use super::upstream_merge::UpstreamMerge;
use crate::domain::{BaseBranch, DiffStat};

pub trait Workspace: Send + Sync {
    fn create_worktree(
        &self,
        repo: &Path,
        worktree: &Path,
        branch: &str,
        start: &StartPoint,
    ) -> Result<(), WorkspaceError>;
    fn remove_worktree(&self, repo: &Path, worktree: &Path) -> Result<(), WorkspaceError>;
    fn unlock_worktree(&self, repo: &Path, worktree: &Path) -> Result<(), WorkspaceError>;
    fn list_worktrees(&self, repo: &Path) -> Result<Vec<WorktreeEntry>, WorkspaceError>;
    fn list_worktree_dirs(&self, root: &Path) -> Result<Vec<PathBuf>, WorkspaceError>;
    fn delete_branch(
        &self,
        repo: &Path,
        branch: &str,
        expected_tip: Option<&str>,
    ) -> Result<(), WorkspaceError>;
    fn rename_branch(&self, location: &Path, old: &str, new: &str) -> Result<(), WorkspaceError>;
    fn drop_stale_upstream(
        &self,
        location: &Path,
        old: &str,
        new: &str,
    ) -> Result<(), WorkspaceError>;
    fn branch_exists(&self, repo: &Path, branch: &str) -> Result<bool, WorkspaceError>;
    fn first_branch_under(
        &self,
        repo: &Path,
        branch: &str,
    ) -> Result<Option<String>, WorkspaceError>;
    fn mark_branch_managed(&self, repo: &Path, branch: &str) -> Result<(), WorkspaceError>;
    fn is_branch_managed(&self, repo: &Path, branch: &str) -> Result<bool, WorkspaceError>;
    fn list_managed_branches(&self, repo: &Path) -> Result<Vec<String>, WorkspaceError>;
    fn is_branch_published(&self, repo: &Path, branch: &str) -> Result<bool, WorkspaceError>;
    fn resolve_start_point(
        &self,
        repo: &Path,
        base_branch: Option<&BaseBranch>,
    ) -> Result<StartPoint, WorkspaceError>;
    fn dirty_state(&self, worktree: &Path) -> Result<DirtyState, WorkspaceError>;
    fn unmerged_commits(
        &self,
        repo: &Path,
        branch: &str,
        worktree: &Path,
    ) -> Result<UnmergedCommits, WorkspaceError>;
    fn diff_stat(&self, worktree: &Path, base: &DiffBase) -> Result<DiffStat, WorkspaceError>;
    fn merge_upstream(
        &self,
        worktree: &Path,
        base_branch: Option<&BaseBranch>,
    ) -> Result<UpstreamMerge, WorkspaceError>;
    fn validate_repo(&self, path: &Path) -> Result<PathBuf, WorkspaceError>;
    fn version(&self) -> Result<String, WorkspaceError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartPoint {
    Commit(String),
    Orphan,
}

impl StartPoint {
    const ORPHAN_BASE_COMMIT: &'static str = "";

    pub fn from_base_commit(base_commit: &str) -> Self {
        match base_commit {
            Self::ORPHAN_BASE_COMMIT => StartPoint::Orphan,
            sha => StartPoint::Commit(sha.to_string()),
        }
    }

    pub fn base_commit(&self) -> &str {
        match self {
            StartPoint::Commit(sha) => sha,
            StartPoint::Orphan => Self::ORPHAN_BASE_COMMIT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    path: PathBuf,
    head: Option<String>,
    branch: Option<String>,
    locked: bool,
    main: bool,
}

impl WorktreeEntry {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            head: None,
            branch: None,
            locked: false,
            main: false,
        }
    }

    pub fn at_head(self, sha: &str) -> Self {
        Self {
            head: Some(sha.to_string()),
            ..self
        }
    }

    pub fn on_branch(self, branch: &str) -> Self {
        Self {
            branch: Some(branch.to_string()),
            ..self
        }
    }

    pub fn locked(self) -> Self {
        Self {
            locked: true,
            ..self
        }
    }

    #[cfg(test)]
    pub fn unlocked(self) -> Self {
        Self {
            locked: false,
            ..self
        }
    }

    pub fn main(self) -> Self {
        Self { main: true, ..self }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn head(&self) -> Option<&str> {
        self.head.as_deref()
    }

    pub fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    pub fn is_main(&self) -> bool {
        self.main
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorkspaceError {
    #[error("git {subcommand} failed ({status}): {stderr}")]
    Failed {
        subcommand: String,
        status: String,
        stderr: String,
    },
    #[error("git {subcommand} timed out after {seconds} seconds")]
    TimedOut { subcommand: String, seconds: u64 },
    #[error("{path} is not a git repository: {detail}")]
    NotARepository { path: PathBuf, detail: String },
    #[error("{path} is a linked worktree, add its main repository instead")]
    LinkedWorktree { path: PathBuf },
    #[error("{path} is a submodule, add its superproject instead")]
    Submodule { path: PathBuf },
    #[error("{repo} has no origin/HEAD, main, or master branch, set base_branch for it")]
    NoBaseBranch { repo: PathBuf },
    #[error(
        "{worktree} has no origin/HEAD, origin/main, or origin/master, set base_branch for its repo"
    )]
    NoUpstream { worktree: PathBuf },
    #[error("{worktree} has a merge in progress, finish or abort it before merging again")]
    MergeInProgress { worktree: PathBuf },
    #[error("{cause}; ran git merge --abort, so the worktree is back where it was")]
    MergeAborted { cause: String },
    #[error(
        "base branch {base} matches neither {local_ref} nor {remote_ref}{}",
        .suggestion.as_ref().map(|name| format!(", did you mean {name}?")).unwrap_or_default()
    )]
    BaseBranchNotFound {
        base: String,
        local_ref: String,
        remote_ref: String,
        suggestion: Option<String>,
    },
}
