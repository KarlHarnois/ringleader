use std::path::Path;

use super::base_branch_ladder::BaseBranchLadder;
use super::git_invocation::GitInvocation;
use super::git_runner::GitRunner;
use crate::application::ports::{DiffBase, StartPoint, WorkspaceError};
use crate::domain::BaseBranch;

pub struct DiffBaseResolver<'a> {
    git: &'a GitRunner,
}

impl<'a> DiffBaseResolver<'a> {
    const HEAD: &'static str = "HEAD";
    const NAME_AND_TARGET_FORMAT: &'static str = "--format=%(refname) %(symref)";

    pub fn new(git: &'a GitRunner) -> Self {
        Self { git }
    }

    pub fn resolve(&self, worktree: &Path, base: &DiffBase) -> Result<String, WorkspaceError> {
        match base.start() {
            StartPoint::Commit(recorded) => {
                self.merge_base_or_recorded(worktree, recorded, base.base_branch())
            }
            StartPoint::Orphan => self.empty_tree(worktree),
        }
    }

    fn merge_base_or_recorded(
        &self,
        worktree: &Path,
        recorded: &str,
        base_branch: Option<&BaseBranch>,
    ) -> Result<String, WorkspaceError> {
        let merge_base = self.merge_base(worktree, recorded, base_branch)?;
        Ok(merge_base.unwrap_or_else(|| recorded.to_string()))
    }

    fn merge_base(
        &self,
        worktree: &Path,
        recorded: &str,
        base_branch: Option<&BaseBranch>,
    ) -> Result<Option<String>, WorkspaceError> {
        let Some(checked_out) = self.checked_out_ref(worktree)? else {
            return Ok(None);
        };
        let branch_refs = self.branch_refs(worktree, base_branch, &checked_out)?;
        let commits = self.existing_commits(worktree, recorded, &branch_refs)?;
        if commits.is_empty() {
            return Ok(None);
        }
        let bases = self.best_common_ancestors(worktree, &commits)?;
        Ok(Self::preferred_base(bases, recorded))
    }

    fn checked_out_ref(&self, worktree: &Path) -> Result<Option<String>, WorkspaceError> {
        let verify = GitInvocation::in_worktree(worktree, "rev-parse")
            .diffing()
            .args(["--verify", "-q", "--symbolic-full-name", Self::HEAD]);
        self.git.resolved_from(&verify)
    }

    fn branch_refs(
        &self,
        worktree: &Path,
        base_branch: Option<&BaseBranch>,
        checked_out: &str,
    ) -> Result<Vec<String>, WorkspaceError> {
        let mut branch_refs = match base_branch {
            Some(branch) => Self::configured_refs(branch),
            None => self.ladder_rung(worktree, checked_out)?,
        };
        branch_refs.retain(|branch_ref| branch_ref != checked_out);
        Ok(branch_refs)
    }

    fn configured_refs(branch: &BaseBranch) -> Vec<String> {
        let origin_copy = branch.origin_ref();
        branch
            .candidate_refs()
            .into_iter()
            .chain([origin_copy])
            .collect()
    }

    fn ladder_rung(
        &self,
        worktree: &Path,
        checked_out: &str,
    ) -> Result<Vec<String>, WorkspaceError> {
        let listed = self.listed_rung_refs(worktree)?;
        let origin_default = listed
            .iter()
            .find(|(name, _)| name == BaseBranchLadder::ORIGIN_HEAD)
            .map(|(_, target)| target.clone());
        let existing_branches: Vec<&str> = listed
            .iter()
            .map(|(name, _)| name.as_str())
            .filter(|name| *name != checked_out)
            .collect();
        Ok(BaseBranchLadder::new(origin_default).first_rung(&existing_branches))
    }

    fn listed_rung_refs(&self, worktree: &Path) -> Result<Vec<(String, String)>, WorkspaceError> {
        let list = GitInvocation::in_worktree(worktree, "for-each-ref")
            .diffing()
            .arg(Self::NAME_AND_TARGET_FORMAT)
            .args(BaseBranchLadder::rung_lookup_refs());
        let listing = self.git.run_checked(&list)?;
        Ok(listing
            .lines()
            .into_iter()
            .filter_map(|line| line.split_once(' '))
            .map(|(name, target)| (name.to_string(), target.to_string()))
            .collect())
    }

    fn existing_commits(
        &self,
        worktree: &Path,
        recorded: &str,
        branch_refs: &[String],
    ) -> Result<Vec<String>, WorkspaceError> {
        let filter = GitInvocation::in_worktree(worktree, "rev-list")
            .diffing()
            .args(["--no-walk", "--ignore-missing", recorded])
            .args(branch_refs)
            .arg("--");
        let commits = self.git.run_checked(&filter)?;
        Ok(commits.lines().into_iter().map(str::to_string).collect())
    }

    fn best_common_ancestors(
        &self,
        worktree: &Path,
        commits: &[String],
    ) -> Result<Vec<String>, WorkspaceError> {
        let find = GitInvocation::in_worktree(worktree, "merge-base")
            .diffing()
            .args(["--all", Self::HEAD])
            .args(commits);
        let bases = self.git.lookup(&find)?;
        Ok(bases
            .map(|found| found.lines().into_iter().map(str::to_string).collect())
            .unwrap_or_default())
    }

    fn preferred_base(bases: Vec<String>, recorded: &str) -> Option<String> {
        let through_the_base_branch = bases.iter().find(|base| *base != recorded).cloned();
        through_the_base_branch.or_else(|| bases.into_iter().next())
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
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::application::ports::Workspace;
    use crate::domain::DiffStat;
    use crate::test_support::GitTestRepo;

    struct DiffBaseFixture {
        repo: GitTestRepo,
        worktree: PathBuf,
        recorded: String,
    }

    impl DiffBaseFixture {
        const SESSION: &'static str = "fix-auth";
        const ORIGIN_DEFAULT: &'static str = "refs/remotes/origin/main";
        const ORIGIN_SHORT_NAME: &'static str = "origin/main";
        const EARLIER: &'static str = "2080-01-01T00:00:00Z";
        const LATER: &'static str = "2090-01-01T00:00:00Z";

        fn started() -> Self {
            Self::started_in(GitTestRepo::new())
        }

        fn started_in(repo: GitTestRepo) -> Self {
            Self::started_on_branch(repo, Self::SESSION)
        }

        fn started_on_branch(repo: GitTestRepo, branch: &str) -> Self {
            let worktree = repo.add_worktree(Self::SESSION, branch);
            let recorded = repo.git(&worktree, &["rev-parse", "HEAD"]);
            Self {
                repo,
                worktree,
                recorded,
            }
        }

        fn started_after_an_unpushed_commit_dated(date: &str) -> Self {
            let repo = GitTestRepo::new();
            Self::commit_dated(&repo, repo.path(), &["local.md"], date);
            Self::started_in(repo)
        }

        fn commit_dated(
            repo: &GitTestRepo,
            directory: &Path,
            files: &[&str],
            date: &str,
        ) -> String {
            for file in files {
                fs::write(directory.join(file), file).unwrap();
                repo.git(directory, &["add", file]);
            }
            repo.shell(&format!(
                "GIT_COMMITTER_DATE={date} git -C '{}' commit -q -m Dated",
                directory.display()
            ));
            repo.git(directory, &["rev-parse", "HEAD"])
        }

        fn repo_on_new_branch(branch: &str) -> GitTestRepo {
            let repo = GitTestRepo::new();
            repo.git(repo.path(), &["checkout", "-q", "-b", branch]);
            repo
        }

        fn repo_developing_ahead_of_main() -> GitTestRepo {
            let repo = Self::repo_on_new_branch("develop");
            repo.commit(repo.path(), "unreleased-1.md");
            let develop = repo.commit(repo.path(), "unreleased-2.md");
            let origin_copy = BaseBranch::new("develop").unwrap().origin_ref();
            repo.git(repo.path(), &["update-ref", &origin_copy, &develop]);
            repo.git(
                repo.path(),
                &["symbolic-ref", BaseBranchLadder::ORIGIN_HEAD, &origin_copy],
            );
            repo
        }

        fn repo_with_only(branch: &str) -> GitTestRepo {
            let repo = GitTestRepo::new();
            repo.git(repo.path(), &["branch", "-m", GitTestRepo::BRANCH, branch]);
            repo
        }

        fn commit_in_session(&self, file: &str) -> String {
            self.repo.commit(&self.worktree, file)
        }

        fn edit_in_session(&self, file: &str) {
            fs::write(self.worktree.join(file), "edited\n").unwrap();
        }

        fn commit_on(&self, branch: &str, file: &str) {
            let repo = self.repo.path();
            self.repo.git(repo, &["checkout", "-q", branch]);
            self.repo.commit(repo, file);
            self.repo.git(repo, &["checkout", "-q", "-"]);
        }

        fn advance_the_checked_out_branch(&self, file: &str) {
            self.repo.commit(self.repo.path(), file);
        }

        fn fast_forward_session(&self, revision: &str) {
            self.repo
                .git(&self.worktree, &["merge", "-q", "--ff-only", revision]);
        }

        fn merge_into_session(&self, revision: &str) {
            self.repo.git(
                &self.worktree,
                &["merge", "-q", "--no-ff", "-m", "Merge", revision],
            );
        }

        fn replace_session_history(&self, file: &str) {
            self.orphan_session();
            self.repo
                .git(&self.worktree, &["rm", "-q", "-r", "-f", "."]);
            self.commit_in_session(file);
        }

        fn orphan_session(&self) {
            self.repo
                .git(&self.worktree, &["checkout", "-q", "--orphan", "fresh"]);
        }

        fn commit_only_on_origin(&self, file: &str) {
            let fetched = self.commit_beside_the_checked_out_branch(file);
            self.point_origin_at(&fetched);
        }

        fn commit_only_on_the_origin_copy_of(&self, branch: &str, file: &str) {
            let fetched = self.commit_beside_the_checked_out_branch(file);
            let origin_copy = BaseBranch::new(branch).unwrap().origin_ref();
            self.repo
                .git(self.repo.path(), &["update-ref", &origin_copy, &fetched]);
        }

        fn commit_beside_the_checked_out_branch(&self, file: &str) -> String {
            let repo = self.repo.path();
            self.repo.git(repo, &["checkout", "-q", "--detach"]);
            let commit = self.repo.commit(repo, file);
            self.repo.git(repo, &["checkout", "-q", "-"]);
            commit
        }

        fn merge_two_upstream_files_dated(&self, date: &str) {
            let repo = self.repo.path();
            self.repo
                .git(repo, &["checkout", "-q", "--detach", "HEAD~1"]);
            let upstream =
                Self::commit_dated(&self.repo, repo, &["upstream-1.md", "upstream-2.md"], date);
            self.repo.git(repo, &["checkout", "-q", "-"]);
            self.point_origin_at(&upstream);
            self.merge_into_session(Self::ORIGIN_SHORT_NAME);
        }

        fn merge_on_origin(&self, tip: &str) {
            let repo = self.repo.path();
            let tree = self
                .repo
                .git(repo, &["rev-parse", &format!("{tip}^{{tree}}")]);
            let merge = self.repo.git(
                repo,
                &[
                    "commit-tree",
                    "-p",
                    GitTestRepo::BRANCH,
                    "-p",
                    tip,
                    "-m",
                    "Merge",
                    &tree,
                ],
            );
            self.point_origin_at(&merge);
        }

        fn point_origin_at(&self, commit: &str) {
            let repo = self.repo.path();
            self.repo
                .git(repo, &["update-ref", Self::ORIGIN_DEFAULT, commit]);
            self.repo.git(
                repo,
                &[
                    "symbolic-ref",
                    BaseBranchLadder::ORIGIN_HEAD,
                    Self::ORIGIN_DEFAULT,
                ],
            );
        }

        fn diff(&self) -> DiffStat {
            self.diff_with_base_branch(None)
        }

        fn diff_with_configured(&self, base_branch: &str) -> DiffStat {
            self.diff_with_base_branch(BaseBranch::new(base_branch).as_ref())
        }

        fn diff_with_base_branch(&self, base_branch: Option<&BaseBranch>) -> DiffStat {
            let base = DiffBase::new(&self.recorded, base_branch);
            self.repo
                .workspace()
                .diff_stat(&self.worktree, &base)
                .unwrap()
        }

        fn line_counts(diff: DiffStat) -> (u64, u64, u64) {
            (diff.files(), diff.insertions(), diff.deletions())
        }
    }

    #[test]
    fn a_fast_forward_to_the_base_branch_counts_only_later_changes() {
        let fixture = DiffBaseFixture::started();
        fixture.advance_the_checked_out_branch("news.md");
        fixture.fast_forward_session(GitTestRepo::BRANCH);
        fixture.commit_in_session("session.md");

        assert_eq!(fixture.diff().files(), 1);
    }

    #[test]
    fn merging_the_base_branch_counts_only_the_session_changes() {
        let fixture = DiffBaseFixture::started();
        fixture.commit_in_session("session.md");
        fixture.advance_the_checked_out_branch("news.md");
        fixture.merge_into_session(GitTestRepo::BRANCH);

        assert_eq!(fixture.diff().files(), 1);
    }

    #[test]
    fn merging_origin_while_local_main_is_stale_counts_only_the_session_changes() {
        let fixture = DiffBaseFixture::started();
        fixture.commit_in_session("session.md");
        fixture.commit_only_on_origin("news.md");
        fixture.merge_into_session(DiffBaseFixture::ORIGIN_SHORT_NAME);

        assert_eq!(fixture.diff().files(), 1);
    }

    #[test]
    fn merging_main_into_a_session_based_on_the_origin_default_keeps_measuring_from_it() {
        let fixture = DiffBaseFixture::started_in(DiffBaseFixture::repo_developing_ahead_of_main());
        fixture.commit_in_session("session.md");
        fixture.commit_on(GitTestRepo::BRANCH, "hotfix.md");
        fixture.merge_into_session(GitTestRepo::BRANCH);

        assert_eq!(fixture.diff().files(), 2);
    }

    #[test]
    fn a_configured_base_branch_counts_only_the_session_changes_after_merging_it() {
        let fixture = DiffBaseFixture::started_in(DiffBaseFixture::repo_on_new_branch("develop"));
        fixture.commit_in_session("session.md");
        fixture.advance_the_checked_out_branch("news.md");
        fixture.merge_into_session("develop");

        assert_eq!(fixture.diff_with_configured("develop").files(), 1);
    }

    #[test]
    fn merging_the_origin_copy_of_a_stale_configured_branch_counts_only_the_session_changes() {
        let fixture = DiffBaseFixture::started_in(DiffBaseFixture::repo_on_new_branch("develop"));
        fixture.commit_in_session("session.md");
        fixture.commit_only_on_the_origin_copy_of("develop", "news.md");
        fixture.merge_into_session("origin/develop");

        assert_eq!(fixture.diff_with_configured("develop").files(), 1);
    }

    #[test]
    fn the_recorded_commit_keeps_a_base_branch_behind_it_from_counting() {
        let repo = DiffBaseFixture::repo_on_new_branch("develop");
        repo.commit(repo.path(), "develop.md");
        let fixture = DiffBaseFixture::started_in(repo);
        fixture.commit_in_session("session.md");

        assert_eq!(fixture.diff().files(), 1);
    }

    #[test]
    fn a_tie_with_a_newer_recorded_commit_measures_from_the_base_branch() {
        let fixture =
            DiffBaseFixture::started_after_an_unpushed_commit_dated(DiffBaseFixture::LATER);
        fixture.commit_in_session("session.md");
        fixture.merge_two_upstream_files_dated(DiffBaseFixture::EARLIER);

        assert_eq!(fixture.diff().files(), 2);
    }

    #[test]
    fn a_tie_with_an_older_recorded_commit_measures_from_the_base_branch() {
        let fixture =
            DiffBaseFixture::started_after_an_unpushed_commit_dated(DiffBaseFixture::EARLIER);
        fixture.commit_in_session("session.md");
        fixture.merge_two_upstream_files_dated(DiffBaseFixture::LATER);

        assert_eq!(fixture.diff().files(), 2);
    }

    #[test]
    fn a_missing_configured_base_branch_falls_back_to_the_recorded_commit() {
        let fixture = DiffBaseFixture::started();
        fixture.commit_in_session("session.md");
        fixture.advance_the_checked_out_branch("news.md");
        fixture.merge_into_session(GitTestRepo::BRANCH);

        assert_eq!(fixture.diff_with_configured("develop").files(), 2);
    }

    #[test]
    fn a_repo_without_a_known_base_branch_measures_from_the_recorded_commit() {
        let fixture = DiffBaseFixture::started_in(DiffBaseFixture::repo_with_only("trunk"));
        fixture.commit_in_session("session.md");
        fixture.advance_the_checked_out_branch("news.md");
        fixture.merge_into_session("trunk");

        assert_eq!(fixture.diff().files(), 2);
    }

    #[test]
    fn a_session_branch_named_like_a_missing_default_branch_still_counts_its_commits() {
        let repo = DiffBaseFixture::repo_with_only("master");
        let fixture = DiffBaseFixture::started_on_branch(repo, "main");
        fixture.commit_in_session("session.md");

        assert_eq!(fixture.diff().files(), 1);
    }

    #[test]
    fn a_session_branch_named_like_a_missing_default_branch_measures_from_the_one_that_exists() {
        let repo = DiffBaseFixture::repo_with_only("master");
        let fixture = DiffBaseFixture::started_on_branch(repo, "main");
        fixture.commit_in_session("session.md");
        fixture.advance_the_checked_out_branch("news.md");
        fixture.merge_into_session("master");

        assert_eq!(fixture.diff().files(), 1);
    }

    #[test]
    fn an_unrelated_history_measures_from_the_recorded_commit() {
        let fixture = DiffBaseFixture::started();
        fixture.replace_session_history("unrelated.md");

        assert_eq!(DiffBaseFixture::line_counts(fixture.diff()), (2, 1, 1));
    }

    #[test]
    fn an_unborn_head_measures_from_the_recorded_commit() {
        let fixture = DiffBaseFixture::started();
        fixture.orphan_session();
        fixture.edit_in_session("README.md");

        assert_eq!(DiffBaseFixture::line_counts(fixture.diff()), (1, 1, 1));
    }

    #[test]
    fn a_branch_merged_into_origin_shows_only_uncommitted_changes() {
        let fixture = DiffBaseFixture::started();
        let tip = fixture.commit_in_session("session.md");
        fixture.merge_on_origin(&tip);
        fixture.edit_in_session("README.md");

        assert_eq!(DiffBaseFixture::line_counts(fixture.diff()), (1, 1, 1));
    }
}
