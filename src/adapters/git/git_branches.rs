use std::path::Path;

use super::branch_marker::BranchMarker;
use super::branch_section::BranchSection;
use super::git_invocation::GitInvocation;
use super::git_runner::GitRunner;
use crate::application::ports::WorkspaceError;

pub struct GitBranches<'a> {
    git: &'a GitRunner,
}

impl<'a> GitBranches<'a> {
    const BRANCH_REFS: &'static str = "refs/heads";
    const REMOTE_REFS: &'static str = "refs/remotes";
    const LOCAL_REMOTE: &'static str = ".";
    const GONE_UPSTREAM: &'static str = "gone";

    pub fn new(git: &'a GitRunner) -> Self {
        Self { git }
    }

    pub fn has_ref(&self, repo: &Path, branch: &str) -> Result<bool, WorkspaceError> {
        Ok(self.git.resolve(repo, &Self::branch_ref(branch))?.is_some())
    }

    pub fn first_under(&self, repo: &Path, branch: &str) -> Result<Option<String>, WorkspaceError> {
        let directory = format!("{}/", Self::branch_ref(branch));
        let list = GitInvocation::in_repo(repo, "for-each-ref").args([
            "--count=1",
            "--format=%(refname:lstrip=2)",
            &directory,
        ]);
        let listing = self.git.run_checked(&list)?;
        Ok(listing.lines().first().map(|line| line.to_string()))
    }

    pub fn delete(
        &self,
        repo: &Path,
        branch: &str,
        expected_tip: Option<&str>,
    ) -> Result<(), WorkspaceError> {
        if let Some(tip) = expected_tip {
            let update = GitInvocation::in_repo(repo, "update-ref").writing().args([
                "-d",
                &Self::branch_ref(branch),
                tip,
            ]);
            self.git.run_checked(&update)?;
        }
        let section = BranchSection::new(branch);
        let probe = GitInvocation::in_repo(repo, "config").args([
            "--local",
            "--get-regexp",
            &section.key_pattern(),
        ]);
        if self.git.run(&probe)?.succeeded() {
            let remove = GitInvocation::in_repo(repo, "config")
                .writing()
                .args(["--remove-section", &section.name()]);
            self.git.run_checked(&remove)?;
        }
        Ok(())
    }

    pub fn rename(&self, location: &Path, old: &str, new: &str) -> Result<(), WorkspaceError> {
        let rename = GitInvocation::in_worktree(location, "branch")
            .writing()
            .args(["-m", old, new]);
        self.git.run_checked(&rename).map(drop)
    }

    pub fn drop_stale_upstream(
        &self,
        location: &Path,
        old: &str,
        new: &str,
    ) -> Result<(), WorkspaceError> {
        let merge_key = BranchSection::new(new).upstream_merge_key();
        if self.read_local_config(location, &merge_key)? != Some(Self::branch_ref(old)) {
            return Ok(());
        }
        let unset = GitInvocation::in_worktree(location, "branch")
            .writing()
            .args(["--unset-upstream", new]);
        self.git.run_checked(&unset).map(drop)
    }

    pub fn mark_managed(&self, repo: &Path, branch: &str) -> Result<(), WorkspaceError> {
        let key = BranchMarker::new(branch).key();
        let mark = GitInvocation::in_repo(repo, "config").writing().args([
            "--local",
            &key,
            BranchMarker::VALUE,
        ]);
        self.git.run_checked(&mark).map(drop)
    }

    pub fn is_managed(&self, repo: &Path, branch: &str) -> Result<bool, WorkspaceError> {
        let value = self.read_local_config(repo, &BranchMarker::new(branch).key())?;
        Ok(value.as_deref() == Some(BranchMarker::VALUE))
    }

    pub fn list_managed(&self, repo: &Path) -> Result<Vec<String>, WorkspaceError> {
        let marked = self.list_marked(repo)?;
        if marked.is_empty() {
            return Ok(marked);
        }
        let existing = self.list_existing(repo)?;
        Ok(marked
            .into_iter()
            .filter(|branch| existing.contains(branch))
            .collect())
    }

    pub fn is_published(&self, repo: &Path, branch: &str) -> Result<bool, WorkspaceError> {
        if !self.has_ref(repo, branch)? {
            return Ok(false);
        }
        if self.has_live_upstream(repo, branch)? {
            return Ok(true);
        }
        for remote_ref in self.remote_copies(repo, branch)? {
            if self.holds_work_pushed_from(repo, branch, &remote_ref)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn list_marked(&self, repo: &Path) -> Result<Vec<String>, WorkspaceError> {
        let list = GitInvocation::in_repo(repo, "config").args([
            "--local",
            "--get-regexp",
            &BranchMarker::listing_pattern(),
        ]);
        let Some(listing) = self.git.lookup(&list)? else {
            return Ok(Vec::new());
        };
        Ok(listing
            .lines()
            .iter()
            .filter_map(|line| line.split_once(' '))
            .filter(|(_, value)| *value == BranchMarker::VALUE)
            .filter_map(|(key, _)| BranchMarker::from_key(key))
            .map(|marker| marker.branch().to_string())
            .collect())
    }

    fn has_live_upstream(&self, repo: &Path, branch: &str) -> Result<bool, WorkspaceError> {
        Ok(self.tracks_its_remote_namesake(repo, branch)?
            && !self.upstream_is_gone(repo, branch)?)
    }

    fn tracks_its_remote_namesake(
        &self,
        repo: &Path,
        branch: &str,
    ) -> Result<bool, WorkspaceError> {
        let section = BranchSection::new(branch);
        let merge = self.read_local_config(repo, &section.upstream_merge_key())?;
        let remote = self.read_local_config(repo, &section.upstream_remote_key())?;
        let on_a_remote = remote.is_some_and(|name| name != Self::LOCAL_REMOTE);
        Ok(on_a_remote && merge == Some(Self::branch_ref(branch)))
    }

    fn upstream_is_gone(&self, repo: &Path, branch: &str) -> Result<bool, WorkspaceError> {
        let read = GitInvocation::in_repo(repo, "for-each-ref").args([
            "--format=%(upstream:track,nobracket)",
            &Self::branch_ref(branch),
        ]);
        let tracking = self.git.run_checked(&read)?;
        Ok(tracking.first_line() == Self::GONE_UPSTREAM)
    }

    fn read_local_config(&self, repo: &Path, key: &str) -> Result<Option<String>, WorkspaceError> {
        let read = GitInvocation::in_repo(repo, "config").args(["--local", "--get", key]);
        let value = self.git.lookup(&read)?;
        Ok(value.map(|found| found.first_line().to_string()))
    }

    fn remote_copies(&self, repo: &Path, branch: &str) -> Result<Vec<String>, WorkspaceError> {
        let remotes = self
            .git
            .run_checked(&GitInvocation::in_repo(repo, "remote"))?;
        let mut copies = Vec::new();
        for remote in remotes.lines() {
            let remote_ref = Self::remote_branch_ref(remote, branch);
            if self.git.resolve(repo, &remote_ref)?.is_some() {
                copies.push(remote_ref);
            }
        }
        Ok(copies)
    }

    fn holds_work_pushed_from(
        &self,
        repo: &Path,
        branch: &str,
        remote_ref: &str,
    ) -> Result<bool, WorkspaceError> {
        let shared = self.merge_bases(repo, &Self::branch_ref(branch), remote_ref)?;
        if shared.is_empty() {
            return Ok(false);
        }
        self.is_ahead_of_the_default_branches(repo, &shared)
    }

    fn merge_bases(
        &self,
        repo: &Path,
        local_ref: &str,
        remote_ref: &str,
    ) -> Result<Vec<String>, WorkspaceError> {
        let find =
            GitInvocation::in_repo(repo, "merge-base").args(["--all", local_ref, remote_ref]);
        let found = self.git.lookup(&find)?;
        Ok(found
            .map(|bases| bases.lines().into_iter().map(str::to_string).collect())
            .unwrap_or_default())
    }

    fn is_ahead_of_the_default_branches(
        &self,
        repo: &Path,
        commits: &[String],
    ) -> Result<bool, WorkspaceError> {
        let default_branches = format!("--glob={}/*/HEAD", Self::REMOTE_REFS);
        let list = GitInvocation::in_repo(repo, "rev-list")
            .arg("-n1")
            .args(commits)
            .args(["--not", &default_branches]);
        Ok(!self.git.run_checked(&list)?.first_line().is_empty())
    }

    fn branch_ref(branch: &str) -> String {
        format!("{}/{branch}", Self::BRANCH_REFS)
    }

    fn remote_branch_ref(remote: &str, branch: &str) -> String {
        format!("{}/{remote}/{branch}", Self::REMOTE_REFS)
    }

    fn list_existing(&self, repo: &Path) -> Result<Vec<String>, WorkspaceError> {
        let list = GitInvocation::in_repo(repo, "for-each-ref")
            .args(["--format=%(refname)", Self::BRANCH_REFS]);
        let listing = self.git.run_checked(&list)?;
        Ok(listing
            .lines()
            .iter()
            .filter_map(|line| line.strip_prefix(Self::BRANCH_REFS))
            .filter_map(|branch| branch.strip_prefix('/'))
            .map(str::to_string)
            .collect())
    }
}
