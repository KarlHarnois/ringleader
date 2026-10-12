use std::path::Path;

use super::git_invocation::GitInvocation;
use super::git_runner::GitRunner;
use crate::application::ports::{BranchMarker, WorkspaceError};

pub struct GitBranches<'a> {
    git: &'a GitRunner,
}

impl<'a> GitBranches<'a> {
    const BRANCH_REFS: &'static str = "refs/heads";
    const REMOTE_REFS: &'static str = "refs/remotes";

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
        let key = BranchMarker::new(branch).key();
        let read = GitInvocation::in_repo(repo, "config").args(["--local", "--get", &key]);
        let value = self.git.lookup(&read)?;
        Ok(value.is_some_and(|found| found.first_line() == BranchMarker::VALUE))
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
        Ok(self.has_upstream(repo, branch)? || self.has_remote_copy(repo, branch)?)
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

    fn has_upstream(&self, repo: &Path, branch: &str) -> Result<bool, WorkspaceError> {
        let key = BranchSection::new(branch).upstream_remote_key();
        let read = GitInvocation::in_repo(repo, "config").args(["--local", "--get", &key]);
        Ok(self.git.lookup(&read)?.is_some())
    }

    fn has_remote_copy(&self, repo: &Path, branch: &str) -> Result<bool, WorkspaceError> {
        let remotes = self
            .git
            .run_checked(&GitInvocation::in_repo(repo, "remote"))?;
        for remote in remotes.lines() {
            let remote_ref = Self::remote_branch_ref(remote, branch);
            if self.git.resolve(repo, &remote_ref)?.is_some() {
                return Ok(true);
            }
        }
        Ok(false)
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

struct BranchSection<'a> {
    branch: &'a str,
}

impl<'a> BranchSection<'a> {
    const REGEX_SPECIAL: &'static str = "\\.^$*+?()[]{}|";

    fn new(branch: &'a str) -> Self {
        Self { branch }
    }

    fn name(&self) -> String {
        format!("branch.{}", self.branch)
    }

    fn upstream_remote_key(&self) -> String {
        format!("{}.remote", self.name())
    }

    fn key_pattern(&self) -> String {
        let escaped: String = self
            .name()
            .chars()
            .flat_map(|character| {
                let escape = Self::REGEX_SPECIAL.contains(character).then_some('\\');
                escape.into_iter().chain([character])
            })
            .collect();
        format!("^{escaped}\\.")
    }
}
