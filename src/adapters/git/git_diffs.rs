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
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::adapters::git::branch_ref::BranchRef;
    use crate::application::base_branch_resolver::BaseBranchResolver;
    use crate::application::diff_base_resolver::DiffBaseResolver;
    use crate::application::ports::Workspace;
    use crate::domain::{BaseBranch, Branch};
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
            let workspace = self.repo.workspace();
            let recorded = StartPoint::from_base_commit(&self.recorded);
            let measured_from = DiffBaseResolver::new(&workspace)
                .resolve(&self.worktree, &recorded, base_branch)
                .unwrap();
            workspace.diff_stat(&self.worktree, &measured_from).unwrap()
        }

        fn diff_requesting(&self, requested: &str) -> DiffStat {
            let workspace = self.repo.workspace();
            let requested = BaseBranch::new(requested).unwrap();
            let base_branch = BaseBranchResolver::new(&workspace)
                .resolve(&self.worktree, Some(&requested), None)
                .unwrap();
            self.diff_with_base_branch(base_branch)
        }

        fn stacked_on(branch: &str) -> Self {
            let repo = Self::repo_on_new_branch(branch);
            repo.commit(repo.path(), "base.md");
            Self::started_in(repo)
        }

        fn land_on_main_beside_other_work(&self, branch: &str) {
            let repo = self.repo.path();
            self.repo
                .git(repo, &["checkout", "-q", GitTestRepo::BRANCH]);
            self.repo
                .git(repo, &["merge", "-q", "--no-ff", "-m", "Land", branch]);
            self.repo.git(repo, &["branch", "-q", "-d", branch]);
            self.repo.commit(repo, "other.md");
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
    fn a_stacked_session_measures_from_main_once_its_base_landed_was_deleted_and_main_merged() {
        let fixture = DiffBaseFixture::stacked_on("part1");
        fixture.commit_in_session("session.md");
        fixture.land_on_main_beside_other_work("part1");
        fixture.merge_into_session(GitTestRepo::BRANCH);

        assert_eq!(fixture.diff_requesting("part1").files(), 1);
    }

    #[test]
    fn a_stacked_session_measures_from_its_base_after_merging_the_base_again() {
        let fixture = DiffBaseFixture::stacked_on("part1");
        fixture.commit_in_session("session.md");
        fixture.commit_on("part1", "base-2.md");
        fixture.merge_into_session("part1");

        assert_eq!(fixture.diff_requesting("part1").files(), 1);
    }

    #[test]
    fn a_session_based_on_develop_never_counts_unreleased_develop_work_after_a_main_hotfix() {
        let repo = DiffBaseFixture::repo_on_new_branch("develop");
        repo.commit(repo.path(), "unreleased-1.md");
        repo.commit(repo.path(), "unreleased-2.md");
        let fixture = DiffBaseFixture::started_in(repo);
        fixture.commit_in_session("session.md");
        fixture.commit_on(GitTestRepo::BRANCH, "hotfix.md");
        fixture.merge_into_session(GitTestRepo::BRANCH);

        assert_eq!(fixture.diff_requesting("develop").files(), 2);
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
