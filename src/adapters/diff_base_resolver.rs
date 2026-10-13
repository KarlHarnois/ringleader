use std::iter;
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
        let Some(head) = self.head(worktree)? else {
            return Ok(None);
        };
        let candidates = self.candidates(worktree, recorded, base_branch)?;
        let commits = self.existing_commits(worktree, &candidates)?;
        if commits.is_empty() {
            return Ok(None);
        }
        self.best_common_ancestor(worktree, &head, &commits)
    }

    fn head(&self, worktree: &Path) -> Result<Option<String>, WorkspaceError> {
        let verify = GitInvocation::in_worktree(worktree, "rev-parse")
            .diffing()
            .args(["--verify", "-q", "HEAD"]);
        self.git.resolved_from(&verify)
    }

    fn candidates(
        &self,
        worktree: &Path,
        recorded: &str,
        base_branch: Option<&BaseBranch>,
    ) -> Result<Vec<String>, WorkspaceError> {
        let branch_refs = match base_branch {
            Some(branch) => Self::configured_refs(branch),
            None => self.origin_ladder(worktree)?.candidates(),
        };
        let lower_bound = iter::once(recorded.to_string());
        Ok(lower_bound.chain(branch_refs).collect())
    }

    fn configured_refs(branch: &BaseBranch) -> Vec<String> {
        let origin_copy = branch.origin_ref();
        branch
            .candidate_refs()
            .into_iter()
            .chain([origin_copy])
            .collect()
    }

    fn origin_ladder(&self, worktree: &Path) -> Result<BaseBranchLadder, WorkspaceError> {
        let origin_head = GitInvocation::in_worktree(worktree, "symbolic-ref")
            .diffing()
            .args(["-q", BaseBranchLadder::ORIGIN_HEAD]);
        Ok(BaseBranchLadder::new(self.git.resolved_from(&origin_head)?))
    }

    fn existing_commits(
        &self,
        worktree: &Path,
        candidates: &[String],
    ) -> Result<Vec<String>, WorkspaceError> {
        let filter = GitInvocation::in_worktree(worktree, "rev-list")
            .diffing()
            .args(["--no-walk", "--ignore-missing"])
            .args(candidates)
            .arg("--");
        let commits = self.git.run_checked(&filter)?;
        Ok(commits.lines().into_iter().map(str::to_string).collect())
    }

    fn best_common_ancestor(
        &self,
        worktree: &Path,
        head: &str,
        commits: &[String],
    ) -> Result<Option<String>, WorkspaceError> {
        let find = GitInvocation::in_worktree(worktree, "merge-base")
            .diffing()
            .arg(head)
            .args(commits);
        self.git.resolved_from(&find)
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

        fn started() -> Self {
            Self::started_in(GitTestRepo::new())
        }

        fn started_in(repo: GitTestRepo) -> Self {
            let worktree = repo.add_worktree(Self::SESSION, Self::SESSION);
            let recorded = repo.git(&worktree, &["rev-parse", "HEAD"]);
            Self {
                repo,
                worktree,
                recorded,
            }
        }

        fn repo_on_new_branch(branch: &str) -> GitTestRepo {
            let repo = GitTestRepo::new();
            repo.git(repo.path(), &["checkout", "-q", "-b", branch]);
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
