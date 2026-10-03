use std::path::Path;

use super::git_invocation::GitInvocation;
use super::git_runner::GitRunner;
use crate::application::ports::{StartPoint, WorkspaceError};
use crate::domain::BaseBranch;

pub struct StartPointResolver<'a> {
    git: &'a GitRunner,
}

impl<'a> StartPointResolver<'a> {
    pub fn new(git: &'a GitRunner) -> Self {
        Self { git }
    }

    pub fn resolve(
        &self,
        repo: &Path,
        base_branch: Option<&BaseBranch>,
    ) -> Result<StartPoint, WorkspaceError> {
        match base_branch {
            Some(base) => self.resolve_explicit(repo, base),
            None => self.resolve_ladder(repo),
        }
    }

    fn resolve_explicit(
        &self,
        repo: &Path,
        base: &BaseBranch,
    ) -> Result<StartPoint, WorkspaceError> {
        for candidate in [base.local_ref(), base.remote_tracking_ref()] {
            if let Some(sha) = self.git.resolve(repo, &format!("{candidate}^{{commit}}"))? {
                return Ok(StartPoint::Commit(sha));
            }
        }
        let suggestion = base.origin_suggestion();
        let origin_ref = format!("refs/remotes/{suggestion}^{{commit}}");
        let origin_exists = self.git.resolve(repo, &origin_ref)?.is_some();
        Err(WorkspaceError::BaseBranchNotFound {
            base: base.to_string(),
            local_ref: base.local_ref(),
            remote_ref: base.remote_tracking_ref(),
            suggestion: origin_exists.then_some(suggestion),
        })
    }

    fn resolve_ladder(&self, repo: &Path) -> Result<StartPoint, WorkspaceError> {
        let origin_head =
            GitInvocation::in_repo(repo, "symbolic-ref").args(["-q", "refs/remotes/origin/HEAD"]);
        let mut candidates: Vec<String> =
            self.git.resolved_from(&origin_head)?.into_iter().collect();
        candidates.extend([
            "refs/heads/main".to_string(),
            "refs/heads/master".to_string(),
        ]);
        for candidate in candidates {
            if let Some(sha) = self.git.resolve(repo, &format!("{candidate}^{{commit}}"))? {
                return Ok(StartPoint::Commit(sha));
            }
        }
        if self.git.resolve(repo, "HEAD")?.is_none() {
            return Ok(StartPoint::Orphan);
        }
        Err(WorkspaceError::NoBaseBranch {
            repo: repo.to_path_buf(),
        })
    }
}
