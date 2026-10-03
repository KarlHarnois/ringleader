use std::path::Path;

use super::git_invocation::GitInvocation;
use super::git_runner::GitRunner;
use crate::application::ports::WorkspaceError;

pub struct GitBranches<'a> {
    git: &'a GitRunner,
}

impl<'a> GitBranches<'a> {
    pub fn new(git: &'a GitRunner) -> Self {
        Self { git }
    }

    pub fn has_ref(&self, repo: &Path, branch: &str) -> Result<bool, WorkspaceError> {
        Ok(self
            .git
            .resolve(repo, &format!("refs/heads/{branch}"))?
            .is_some())
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
                &format!("refs/heads/{branch}"),
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

    pub fn list(&self, repo: &Path, prefix: &str) -> Result<Vec<String>, WorkspaceError> {
        let pattern = format!("refs/heads/{}", prefix.trim_end_matches('/'));
        let list =
            GitInvocation::in_repo(repo, "for-each-ref").args(["--format=%(refname)", &pattern]);
        let listing = self.git.run_checked(&list)?;
        Ok(listing
            .lines()
            .iter()
            .filter_map(|line| line.strip_prefix("refs/heads/"))
            .filter(|branch| branch.starts_with(prefix))
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
