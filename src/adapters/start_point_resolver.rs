use std::path::Path;

use super::branch_ref::BranchRef;
use super::git_refs::GitRefs;
use super::git_runner::GitRunner;
use crate::application::ports::{StartPoint, WorkspaceError};
use crate::domain::{BaseBranch, BaseBranchLadder, Branch};

pub struct StartPointResolver<'a> {
    refs: GitRefs<'a>,
}

impl<'a> StartPointResolver<'a> {
    pub fn new(git: &'a GitRunner) -> Self {
        Self {
            refs: GitRefs::new(git),
        }
    }

    pub fn resolve(
        &self,
        repo: &Path,
        base_branch: Option<&BaseBranch>,
    ) -> Result<StartPoint, WorkspaceError> {
        match base_branch {
            Some(base) => self.resolve_configured(repo, base),
            None => self.resolve_ladder(repo),
        }
    }

    fn resolve_configured(
        &self,
        repo: &Path,
        base: &BaseBranch,
    ) -> Result<StartPoint, WorkspaceError> {
        let tips = self
            .refs
            .branch_tips(repo, &base.branches_and_origin_copy())?;
        if let Some(commit) = tips.first_of(&base.branches()) {
            return Ok(StartPoint::Commit(commit.to_string()));
        }
        let [local, remote_tracking] = base.branches();
        let origin_copy = base.origin_copy();
        Err(WorkspaceError::BaseBranchNotFound {
            base: base.to_string(),
            local_ref: BranchRef::new(&local).full_name(),
            remote_ref: BranchRef::new(&remote_tracking).full_name(),
            suggestion: tips
                .tip_of(&origin_copy)
                .map(|_| origin_copy.name().to_string()),
        })
    }

    fn resolve_ladder(&self, repo: &Path) -> Result<StartPoint, WorkspaceError> {
        let origin_default = self.refs.remote_default_branch(repo, Branch::ORIGIN)?;
        let candidates = BaseBranchLadder::new(origin_default).start_candidates();
        let tips = self.refs.branch_tips(repo, &candidates)?;
        if let Some(commit) = tips.first_of(&candidates) {
            return Ok(StartPoint::Commit(commit.to_string()));
        }
        if self.refs.head(repo)?.is_unborn() {
            return Ok(StartPoint::Orphan);
        }
        Err(WorkspaceError::NoBaseBranch {
            repo: repo.to_path_buf(),
        })
    }
}
