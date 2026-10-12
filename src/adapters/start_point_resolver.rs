use std::path::Path;

use super::git_invocation::GitInvocation;
use super::git_runner::GitRunner;
use crate::application::ports::{StartPoint, WorkspaceError};
use crate::domain::BaseBranch;

pub struct StartPointResolver<'a> {
    git: &'a GitRunner,
}

impl<'a> StartPointResolver<'a> {
    const ORIGIN_HEAD: &'static str = "refs/remotes/origin/HEAD";
    const ORIGIN_BRANCH_PREFIX: &'static str = "refs/remotes/origin/";
    const CONVENTIONAL_DEFAULT_BRANCHES: [&'static str; 2] =
        ["refs/heads/main", "refs/heads/master"];

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
        let candidates = [base.local_ref(), base.remote_tracking_ref()];
        if let Some(sha) = self.first_commit(repo, candidates)? {
            return Ok(StartPoint::Commit(sha));
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
        let candidates = self.ladder_candidates(repo)?;
        if let Some(sha) = self.first_commit(repo, candidates)? {
            return Ok(StartPoint::Commit(sha));
        }
        if self.git.resolve(repo, "HEAD")?.is_none() {
            return Ok(StartPoint::Orphan);
        }
        Err(WorkspaceError::NoBaseBranch {
            repo: repo.to_path_buf(),
        })
    }

    fn ladder_candidates(&self, repo: &Path) -> Result<Vec<String>, WorkspaceError> {
        let mut candidates = self.origin_default_candidates(repo)?;
        candidates.extend(Self::CONVENTIONAL_DEFAULT_BRANCHES.map(String::from));
        Ok(candidates)
    }

    fn origin_default_candidates(&self, repo: &Path) -> Result<Vec<String>, WorkspaceError> {
        let Some(origin_default) = self.existing_origin_default(repo)? else {
            return Ok(Vec::new());
        };
        let local_default = Self::local_counterpart(&origin_default);
        Ok(local_default.into_iter().chain([origin_default]).collect())
    }

    fn existing_origin_default(&self, repo: &Path) -> Result<Option<String>, WorkspaceError> {
        let origin_head =
            GitInvocation::in_repo(repo, "symbolic-ref").args(["-q", Self::ORIGIN_HEAD]);
        let Some(origin_default) = self.git.resolved_from(&origin_head)? else {
            return Ok(None);
        };
        let exists = self.commit_of(repo, &origin_default)?.is_some();
        Ok(exists.then_some(origin_default))
    }

    fn local_counterpart(origin_ref: &str) -> Option<String> {
        origin_ref
            .strip_prefix(Self::ORIGIN_BRANCH_PREFIX)
            .and_then(BaseBranch::new)
            .map(|branch| branch.local_ref())
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
