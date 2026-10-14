use std::iter;
use std::path::Path;

use super::git_diffs::GitDiffs;
use super::git_refs::GitRefs;
use super::git_runner::GitRunner;
use crate::application::ports::{StartPoint, WorkspaceError};
use crate::domain::{BaseBranch, BaseBranchLadder, Branch, BranchTips};

pub struct DiffBaseResolver<'a> {
    refs: GitRefs<'a>,
    diffs: GitDiffs<'a>,
}

impl<'a> DiffBaseResolver<'a> {
    pub fn new(git: &'a GitRunner) -> Self {
        Self {
            refs: GitRefs::new(git),
            diffs: GitDiffs::new(git),
        }
    }

    pub fn resolve(
        &self,
        worktree: &Path,
        recorded: &StartPoint,
        base_branch: Option<&BaseBranch>,
    ) -> Result<StartPoint, WorkspaceError> {
        match recorded {
            StartPoint::Commit(commit) => self
                .measured_from(worktree, commit, base_branch)
                .map(StartPoint::Commit),
            StartPoint::Orphan => Ok(StartPoint::Orphan),
        }
    }

    fn measured_from(
        &self,
        worktree: &Path,
        recorded: &str,
        base_branch: Option<&BaseBranch>,
    ) -> Result<String, WorkspaceError> {
        let head = self.refs.head(worktree)?;
        if head.is_unborn() {
            return Ok(recorded.to_string());
        }
        let base_commits = self.base_branch_commits(worktree, base_branch, head.branch())?;
        let candidates: Vec<String> = iter::once(recorded.to_string())
            .chain(base_commits)
            .collect();
        let bases = self.diffs.merge_bases(worktree, &candidates)?;
        Ok(bases.measured_from(recorded).to_string())
    }

    fn base_branch_commits(
        &self,
        worktree: &Path,
        base_branch: Option<&BaseBranch>,
        checked_out: Option<&Branch>,
    ) -> Result<Vec<String>, WorkspaceError> {
        match base_branch {
            Some(base) => self.configured_commits(worktree, base, checked_out),
            None => self.ladder_commits(worktree, checked_out),
        }
    }

    fn configured_commits(
        &self,
        worktree: &Path,
        base: &BaseBranch,
        checked_out: Option<&Branch>,
    ) -> Result<Vec<String>, WorkspaceError> {
        let rung = base.branches_and_origin_copy();
        let tips = self.refs.branch_tips(worktree, &rung)?;
        Ok(Self::commits_besides(&tips, rung, checked_out))
    }

    fn ladder_commits(
        &self,
        worktree: &Path,
        checked_out: Option<&Branch>,
    ) -> Result<Vec<String>, WorkspaceError> {
        let origin_default = self.refs.remote_default_branch(worktree, Branch::ORIGIN)?;
        let ladder = BaseBranchLadder::new(origin_default);
        let tips = self.refs.branch_tips(worktree, &ladder.diff_lookups())?;
        let rung = ladder.diff_rung(&tips, checked_out);
        Ok(Self::commits_besides(&tips, rung, checked_out))
    }

    fn commits_besides(
        tips: &BranchTips,
        mut rung: Vec<Branch>,
        checked_out: Option<&Branch>,
    ) -> Vec<String> {
        rung.retain(|branch| Some(branch) != checked_out);
        tips.commits_of(&rung)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::adapters::branch_ref::BranchRef;
    use crate::application::ports::{DiffBase, Workspace};
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
        const ORIGIN_HEAD: &'static str = "refs/remotes/origin/HEAD";
        const EARLIER: &'static str = "2080-01-01T00:00:00Z";
        const LATER: &'static str = "2090-01-01T00:00:00Z";

        fn origin_ref(branch: &str) -> String {
            BranchRef::new(&Branch::origin_copy_of(branch)).full_name()
        }

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
            let origin_copy = Self::origin_ref("develop");
            repo.git(repo.path(), &["update-ref", &origin_copy, &develop]);
            repo.git(
                repo.path(),
                &["symbolic-ref", Self::ORIGIN_HEAD, &origin_copy],
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

        fn make_origin_head_a_plain_ref(&self) {
            let repo = self.repo.path();
            let tip = self.repo.git(repo, &["rev-parse", "HEAD"]);
            self.repo
                .git(repo, &["update-ref", "--no-deref", Self::ORIGIN_HEAD, &tip]);
        }

        fn leave_a_pruned_origin_default_beside_its_local_branch(&self, branch: &str) {
            let repo = self.repo.path();
            self.repo.git(repo, &["branch", branch]);
            let pruned = Self::origin_ref(branch);
            self.repo
                .git(repo, &["symbolic-ref", Self::ORIGIN_HEAD, &pruned]);
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
            let origin_copy = Self::origin_ref(branch);
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
                &["symbolic-ref", Self::ORIGIN_HEAD, Self::ORIGIN_DEFAULT],
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
    fn a_plain_origin_head_leaves_main_as_the_base_branch() {
        let fixture = DiffBaseFixture::started();
        fixture.make_origin_head_a_plain_ref();
        fixture.commit_in_session("session.md");
        fixture.advance_the_checked_out_branch("news.md");
        fixture.merge_into_session(GitTestRepo::BRANCH);

        assert_eq!(fixture.diff().files(), 1);
    }

    #[test]
    fn a_dangling_origin_head_leaves_main_as_the_base_branch() {
        let fixture = DiffBaseFixture::started();
        fixture.leave_a_pruned_origin_default_beside_its_local_branch("master");
        fixture.commit_in_session("session.md");
        fixture.advance_the_checked_out_branch("news.md");
        fixture.merge_into_session(GitTestRepo::BRANCH);

        assert_eq!(fixture.diff().files(), 1);
    }

    #[test]
    fn merging_origin_main_without_an_origin_head_counts_only_the_session_changes() {
        let fixture = DiffBaseFixture::started();
        fixture.commit_in_session("session.md");
        fixture.commit_only_on_the_origin_copy_of(GitTestRepo::BRANCH, "news.md");
        fixture.merge_into_session(DiffBaseFixture::ORIGIN_SHORT_NAME);

        assert_eq!(fixture.diff().files(), 1);
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
    fn a_session_branch_named_like_the_origin_default_still_counts_its_commits() {
        let repo = DiffBaseFixture::repo_with_only("master");
        let fixture = DiffBaseFixture::started_on_branch(repo, "main");
        fixture.point_origin_at(&fixture.recorded);
        fixture.commit_in_session("session.md");

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
