use std::path::Path;

use super::git_invocation::GitInvocation;
use super::git_runner::GitRunner;
use crate::application::ports::{StartPoint, WorkspaceError};
use crate::domain::{DiffStat, MergeBases};

pub struct GitDiffs<'a> {
    git: &'a GitRunner,
}

impl<'a> GitDiffs<'a> {
    const HEAD: &'static str = "HEAD";

    pub fn new(git: &'a GitRunner) -> Self {
        Self { git }
    }

    pub fn merge_bases(
        &self,
        worktree: &Path,
        commits: &[String],
    ) -> Result<MergeBases, WorkspaceError> {
        let existing = self.existing_commits(worktree, commits)?;
        if existing.is_empty() {
            return Ok(MergeBases::default());
        }
        let find = GitInvocation::in_worktree(worktree, "merge-base")
            .diffing()
            .args(["--all", Self::HEAD])
            .args(&existing);
        let bases = self.git.lookup(&find)?;
        Ok(MergeBases::new(bases.map_or_else(Vec::new, |found| {
            found.lines().into_iter().map(str::to_string).collect()
        })))
    }

    pub fn diff_stat(
        &self,
        worktree: &Path,
        base: &StartPoint,
    ) -> Result<DiffStat, WorkspaceError> {
        let base_commit = self.base_commit(worktree, base)?;
        let numstat = GitInvocation::in_worktree(worktree, "diff-index")
            .diffing()
            .args(["--numstat", "--no-renames", &base_commit]);
        let untracked = GitInvocation::in_worktree(worktree, "ls-files")
            .diffing()
            .args(["--others", "--exclude-standard", "-z"]);
        let changes = self.git.run_checked(&numstat)?.file_changes();
        let untracked_count = self.git.run_checked(&untracked)?.record_count();
        Ok(DiffStat::from_changes(changes, untracked_count))
    }

    fn existing_commits(
        &self,
        worktree: &Path,
        commits: &[String],
    ) -> Result<Vec<String>, WorkspaceError> {
        if commits.is_empty() {
            return Ok(Vec::new());
        }
        let filter = GitInvocation::in_worktree(worktree, "rev-list")
            .diffing()
            .args(["--no-walk", "--ignore-missing"])
            .args(commits)
            .arg("--");
        let listed = self.git.run_checked(&filter)?;
        Ok(listed.lines().into_iter().map(str::to_string).collect())
    }

    fn base_commit(&self, worktree: &Path, base: &StartPoint) -> Result<String, WorkspaceError> {
        match base {
            StartPoint::Commit(commit) => Ok(commit.clone()),
            StartPoint::Orphan => self.empty_tree(worktree),
        }
    }

    fn empty_tree(&self, worktree: &Path) -> Result<String, WorkspaceError> {
        let hash = GitInvocation::in_worktree(worktree, "hash-object")
            .diffing()
            .args(["-t", "tree", "/dev/null"]);
        Ok(self.git.run_checked(&hash)?.first_line().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::GitTestRepo;

    struct HistoryFixture {
        repo: GitTestRepo,
        git: GitRunner,
    }

    impl HistoryFixture {
        const RECORDED: &'static str = "recorded";
        const MISSING: &'static str = "0123456789abcdef0123456789abcdef01234567";

        fn new() -> Self {
            let repo = GitTestRepo::new();
            let git = repo.runner();
            Self { repo, git }
        }

        fn run(&self, args: &[&str]) -> String {
            self.repo.git(self.repo.path(), args)
        }

        fn commit(&self, file: &str) -> String {
            self.repo.commit(self.repo.path(), file)
        }

        fn merge(&self, revision: &str) -> String {
            self.run(&["merge", "-q", "--no-ff", "-m", "Merge", revision]);
            self.run(&["rev-parse", "HEAD"])
        }

        fn merge_bases(&self, commits: &[&str]) -> MergeBases {
            let commits: Vec<String> = commits.iter().map(ToString::to_string).collect();
            GitDiffs::new(&self.git)
                .merge_bases(self.repo.path(), &commits)
                .unwrap()
        }

        fn criss_cross(&self) -> (String, String, String) {
            self.run(&["checkout", "-q", "-b", "left"]);
            let left = self.commit("left.md");
            self.run(&["checkout", "-q", "-b", "right", "HEAD~1"]);
            let right = self.commit("right.md");
            let right_merge = self.merge(&left);
            self.run(&["checkout", "-q", "left"]);
            self.merge(&right);
            (left, right, right_merge)
        }
    }

    #[test]
    fn a_missing_commit_is_skipped() {
        let fixture = HistoryFixture::new();
        let tip = fixture.run(&["rev-parse", "HEAD"]);

        let bases = fixture.merge_bases(&[HistoryFixture::MISSING, &tip]);

        assert_eq!(bases.measured_from(HistoryFixture::RECORDED), tip);
    }

    #[test]
    fn an_unrelated_history_has_no_merge_base() {
        let fixture = HistoryFixture::new();
        fixture.run(&["checkout", "-q", "--orphan", "unrelated"]);
        let unrelated = fixture.commit("unrelated.md");
        fixture.run(&["checkout", "-q", GitTestRepo::BRANCH]);

        let bases = fixture.merge_bases(&[&unrelated]);

        assert_eq!(bases, MergeBases::default());
    }

    #[test]
    fn a_criss_cross_merge_has_both_sides_as_merge_bases() {
        let fixture = HistoryFixture::new();
        let (left, right, right_merge) = fixture.criss_cross();

        let bases = fixture.merge_bases(&[&right_merge]);

        assert_eq!(bases.measured_from(&left), right);
        assert_eq!(bases.measured_from(&right), left);
    }

    #[test]
    fn asking_about_no_commit_finds_no_merge_base() {
        let fixture = HistoryFixture::new();

        assert_eq!(fixture.merge_bases(&[]), MergeBases::default());
    }
}
