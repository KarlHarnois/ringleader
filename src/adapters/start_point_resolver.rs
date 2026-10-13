use std::path::Path;

use super::base_branch_ladder::BaseBranchLadder;
use super::base_branch_refs::BaseBranchRefs;
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
        let refs = BaseBranchRefs::new(base);
        if let Some(sha) = self.first_commit(repo, refs.candidate_refs())? {
            return Ok(StartPoint::Commit(sha));
        }
        let origin_exists = self.commit_of(repo, &refs.origin_ref())?.is_some();
        Err(WorkspaceError::BaseBranchNotFound {
            base: base.to_string(),
            local_ref: refs.local_ref(),
            remote_ref: refs.remote_tracking_ref(),
            suggestion: origin_exists.then(|| refs.origin_suggestion()),
        })
    }

    fn resolve_ladder(&self, repo: &Path) -> Result<StartPoint, WorkspaceError> {
        let ladder = BaseBranchLadder::new(self.existing_origin_default(repo)?);
        if let Some(sha) = self.first_commit(repo, ladder.candidates())? {
            return Ok(StartPoint::Commit(sha));
        }
        if self.git.resolve(repo, "HEAD")?.is_none() {
            return Ok(StartPoint::Orphan);
        }
        Err(WorkspaceError::NoBaseBranch {
            repo: repo.to_path_buf(),
        })
    }

    fn existing_origin_default(&self, repo: &Path) -> Result<Option<String>, WorkspaceError> {
        let origin_head = GitInvocation::in_repo(repo, "symbolic-ref")
            .args(["-q", BaseBranchLadder::ORIGIN_HEAD]);
        let Some(origin_default) = self.git.resolved_from(&origin_head)? else {
            return Ok(None);
        };
        let exists = self.commit_of(repo, &origin_default)?.is_some();
        Ok(exists.then_some(origin_default))
    }

    fn first_commit(
        &self,
        repo: &Path,
        candidates: impl IntoIterator<Item = String>,
    ) -> Result<Option<String>, WorkspaceError> {
        for candidate in candidates {
            if let Some(sha) = self.commit_of(repo, &candidate)? {
                return Ok(Some(sha));
            }
        }
        Ok(None)
    }

    fn commit_of(&self, repo: &Path, reference: &str) -> Result<Option<String>, WorkspaceError> {
        self.git.resolve(repo, &format!("{reference}^{{commit}}"))
    }
}
