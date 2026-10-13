use std::path::Path;

use super::base_branch_ladder::BaseBranchLadder;
use super::git_invocation::GitInvocation;
use super::git_runner::GitRunner;
use super::start_point_resolver::StartPointResolver;
use crate::application::ports::{MergeOutcome, UpstreamMerge, WorkspaceError};
use crate::domain::BaseBranch;

pub struct UpstreamMerger<'a> {
    git: &'a GitRunner,
}

impl<'a> UpstreamMerger<'a> {
    const REMOTE: &'static str = "origin";
    const MERGE_HEAD: &'static str = "MERGE_HEAD";

    pub fn new(git: &'a GitRunner) -> Self {
        Self { git }
    }

    pub fn merge(
        &self,
        worktree: &Path,
        base_branch: Option<&BaseBranch>,
    ) -> Result<UpstreamMerge, WorkspaceError> {
        let branch = self.upstream_branch(worktree, base_branch)?;
        self.fetch(worktree, &branch)?;
        let origin_ref = branch.origin_ref();
        let commit = self.commit_of(worktree, &origin_ref)?;
        let outcome = self.merge_unless_contained(worktree, &origin_ref, &commit)?;
        Ok(UpstreamMerge::new(&branch.origin_suggestion(), outcome))
    }

    fn upstream_branch(
        &self,
        worktree: &Path,
        base_branch: Option<&BaseBranch>,
    ) -> Result<BaseBranch, WorkspaceError> {
        let branch = match base_branch {
            Some(base) => base.on_origin(),
            None => self.origin_default(worktree)?,
        };
        branch.ok_or_else(|| WorkspaceError::NoUpstream {
            worktree: worktree.to_path_buf(),
        })
    }

    fn origin_default(&self, worktree: &Path) -> Result<Option<BaseBranch>, WorkspaceError> {
        let origin_head = StartPointResolver::new(self.git).existing_origin_default(worktree)?;
        for candidate in BaseBranchLadder::new(origin_head).origin_candidates() {
            if self.git.resolve(worktree, &candidate)?.is_some() {
                return Ok(BaseBranchLadder::origin_branch(&candidate));
            }
        }
        Ok(None)
    }

    fn fetch(&self, worktree: &Path, branch: &BaseBranch) -> Result<(), WorkspaceError> {
        let refspec = format!("+{}:{}", branch.local_ref(), branch.origin_ref());
        let fetch = GitInvocation::in_worktree(worktree, "fetch")
            .writing()
            .args([Self::REMOTE, &refspec]);
        self.git.run_checked(&fetch).map(drop)
    }

    fn commit_of(&self, worktree: &Path, reference: &str) -> Result<String, WorkspaceError> {
        let verify = GitInvocation::in_worktree(worktree, "rev-parse")
            .args(["--verify", &format!("{reference}^{{commit}}")]);
        Ok(self.git.run_checked(&verify)?.first_line().to_string())
    }

    fn merge_unless_contained(
        &self,
        worktree: &Path,
        origin_ref: &str,
        commit: &str,
    ) -> Result<MergeOutcome, WorkspaceError> {
        if self.head_contains(worktree, commit)? {
            return Ok(MergeOutcome::UpToDate);
        }
        let merge = GitInvocation::in_worktree(worktree, "merge")
            .writing()
            .args(["--no-edit", origin_ref]);
        match self.git.run_checked(&merge) {
            Ok(_) => Ok(MergeOutcome::Merged),
            Err(failure) => self.conflicts_or(worktree, failure),
        }
    }

    fn head_contains(&self, worktree: &Path, commit: &str) -> Result<bool, WorkspaceError> {
        let ancestry = GitInvocation::in_worktree(worktree, "merge-base").args([
            "--is-ancestor",
            commit,
            "HEAD",
        ]);
        Ok(self.git.lookup(&ancestry)?.is_some())
    }

    fn conflicts_or(
        &self,
        worktree: &Path,
        failure: WorkspaceError,
    ) -> Result<MergeOutcome, WorkspaceError> {
        let unmerged = GitInvocation::in_worktree(worktree, "diff").args([
            "--name-only",
            "--diff-filter=U",
            "-z",
        ]);
        let conflicts = self.git.run_checked(&unmerged)?.paths();
        if conflicts.is_empty() {
            return Err(self.undo_unfinished_merge(worktree, failure)?);
        }
        Ok(MergeOutcome::Conflicted(conflicts))
    }

    fn undo_unfinished_merge(
        &self,
        worktree: &Path,
        failure: WorkspaceError,
    ) -> Result<WorkspaceError, WorkspaceError> {
        if self.git.resolve(worktree, Self::MERGE_HEAD)?.is_none() {
            return Ok(failure);
        }
        let abort = GitInvocation::in_worktree(worktree, "merge")
            .writing()
            .arg("--abort");
        self.git.run_checked(&abort)?;
        Ok(WorkspaceError::MergeAborted {
            cause: failure.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    use super::*;
    use crate::application::ports::Workspace;
    use crate::test_support::GitTestRepo;

    struct MergeFixture {
        origin: GitTestRepo,
        clone: PathBuf,
        worktree: PathBuf,
    }

    impl MergeFixture {
        const BRANCH: &'static str = "fix-auth";
        const SHARED_FILE: &'static str = "shared.txt";
        const EXECUTABLE: u32 = 0o755;

        fn new() -> Self {
            let origin = GitTestRepo::new();
            let clone = origin.sibling("clone");
            origin.git(
                origin.root(),
                &[
                    "clone",
                    "-q",
                    &Self::text(origin.path()),
                    &Self::text(&clone),
                ],
            );
            let worktree = origin.sibling("worktree");
            origin.git(
                &clone,
                &[
                    "worktree",
                    "add",
                    "-q",
                    "-b",
                    Self::BRANCH,
                    &Self::text(&worktree),
                ],
            );
            Self {
                origin,
                clone,
                worktree,
            }
        }

        fn merge(&self) -> Result<UpstreamMerge, WorkspaceError> {
            self.merge_from(None)
        }

        fn merge_from(&self, base: Option<&BaseBranch>) -> Result<UpstreamMerge, WorkspaceError> {
            self.origin.workspace().merge_upstream(&self.worktree, base)
        }

        fn commit_upstream(&self, file: &str) -> String {
            self.origin.commit(self.origin.path(), file)
        }

        fn commit_on_branch(&self, file: &str) -> String {
            self.origin.commit(&self.worktree, file)
        }

        fn head(&self) -> String {
            self.origin.git(&self.worktree, &["rev-parse", "HEAD"])
        }

        fn has_merged(&self, commit: &str) -> bool {
            let ancestry = self.origin.try_git(
                &self.worktree,
                &["merge-base", "--is-ancestor", commit, "HEAD"],
            );
            ancestry.status.success()
        }

        fn is_merging(&self) -> bool {
            let merge_head = self.origin.try_git(
                &self.worktree,
                &["rev-parse", "-q", "--verify", "MERGE_HEAD"],
            );
            merge_head.status.success()
        }

        fn upstream_branch(&self, branch: &str) -> String {
            let repo = self.origin.path();
            self.origin.git(repo, &["checkout", "-q", "-b", branch]);
            let tip = self.origin.commit(repo, "develop.txt");
            self.origin
                .git(repo, &["checkout", "-q", GitTestRepo::BRANCH]);
            tip
        }

        fn without_origin_head(self) -> Self {
            self.origin
                .git(&self.clone, &["remote", "set-head", "origin", "-d"]);
            self
        }

        fn without_origin_default(self) -> Self {
            let fixture = self.without_origin_head();
            let tracking = format!("refs/remotes/origin/{}", GitTestRepo::BRANCH);
            fixture
                .origin
                .git(&fixture.clone, &["update-ref", "-d", &tracking]);
            fixture
        }

        fn with_local_branch_shadowing_origin(self) -> Self {
            let shadow = format!("origin/{}", GitTestRepo::BRANCH);
            self.origin.git(&self.clone, &["branch", &shadow]);
            self
        }

        fn reject_commit_messages(&self) {
            let hook = self.clone.join(".git/hooks/commit-msg");
            fs::write(&hook, "#!/bin/sh\necho 'needs a ticket id' >&2\nexit 1\n").unwrap();
            fs::set_permissions(&hook, fs::Permissions::from_mode(Self::EXECUTABLE)).unwrap();
        }

        fn is_clean(&self) -> bool {
            self.origin
                .git(&self.worktree, &["status", "--porcelain"])
                .is_empty()
        }

        fn without_remote(self) -> Self {
            self.origin
                .git(&self.clone, &["remote", "remove", "origin"]);
            self
        }

        fn conflicting_edits(&self) {
            self.commit_upstream(Self::SHARED_FILE);
            self.commit_on_branch(Self::SHARED_FILE);
        }

        fn conflicted_paths(merge: &UpstreamMerge) -> Vec<String> {
            match merge.outcome() {
                MergeOutcome::Conflicted(paths) => paths.clone(),
                other => panic!("expected conflicts, got {other:?}"),
            }
        }

        fn text(path: &Path) -> String {
            path.to_string_lossy().into_owned()
        }
    }

    #[test]
    fn merges_a_new_origin_commit_into_the_worktree() {
        let fixture = MergeFixture::new();
        fixture.commit_on_branch("work.txt");
        let upstream_tip = fixture.commit_upstream("news.txt");

        let merge = fixture.merge().unwrap();

        assert_eq!(
            merge,
            UpstreamMerge::new("origin/main", MergeOutcome::Merged)
        );
        assert!(fixture.has_merged(&upstream_tip));
    }

    #[test]
    fn an_origin_with_nothing_new_is_up_to_date_and_adds_no_commit() {
        let fixture = MergeFixture::new();
        fixture.commit_on_branch("work.txt");
        let head = fixture.head();

        let merge = fixture.merge().unwrap();

        assert_eq!(merge.outcome(), &MergeOutcome::UpToDate);
        assert_eq!(fixture.head(), head);
    }

    #[test]
    fn conflicting_edits_report_the_conflicted_paths() {
        let fixture = MergeFixture::new();
        fixture.conflicting_edits();

        let merge = fixture.merge().unwrap();

        assert_eq!(
            MergeFixture::conflicted_paths(&merge),
            [MergeFixture::SHARED_FILE]
        );
    }

    #[test]
    fn a_conflicted_merge_stays_in_progress() {
        let fixture = MergeFixture::new();
        fixture.conflicting_edits();

        fixture.merge().unwrap();

        assert!(fixture.is_merging());
    }

    #[test]
    fn a_configured_base_branch_merges_its_origin_copy() {
        let fixture = MergeFixture::new();
        let develop_tip = fixture.upstream_branch("develop");
        let base = BaseBranch::new("develop").unwrap();

        let merge = fixture.merge_from(Some(&base)).unwrap();

        assert_eq!(
            merge,
            UpstreamMerge::new("origin/develop", MergeOutcome::Merged)
        );
        assert!(fixture.has_merged(&develop_tip));
    }

    #[test]
    fn an_origin_tracking_base_branch_merges_the_same_branch() {
        let fixture = MergeFixture::new();
        let develop_tip = fixture.upstream_branch("develop");
        let base = BaseBranch::new("origin/develop").unwrap();

        let merge = fixture.merge_from(Some(&base)).unwrap();

        assert_eq!(merge.upstream(), "origin/develop");
        assert!(fixture.has_merged(&develop_tip));
    }

    #[test]
    fn without_origin_head_the_conventional_default_branch_is_merged() {
        let fixture = MergeFixture::new().without_origin_head();
        let upstream_tip = fixture.commit_upstream("news.txt");

        let merge = fixture.merge().unwrap();

        assert_eq!(merge.upstream(), "origin/main");
        assert!(fixture.has_merged(&upstream_tip));
    }

    #[test]
    fn a_local_branch_named_like_the_origin_copy_does_not_shadow_it() {
        let fixture = MergeFixture::new().with_local_branch_shadowing_origin();
        fixture.commit_on_branch("work.txt");
        let upstream_tip = fixture.commit_upstream("news.txt");

        fixture.merge().unwrap();

        assert!(fixture.has_merged(&upstream_tip));
    }

    #[test]
    fn a_merge_commit_refused_by_a_hook_is_undone() {
        let fixture = MergeFixture::new();
        fixture.commit_on_branch("work.txt");
        fixture.commit_upstream("news.txt");
        fixture.reject_commit_messages();

        let error = fixture.merge().unwrap_err();

        assert!(
            matches!(error, WorkspaceError::MergeAborted { .. }),
            "{error:?}"
        );
        assert!(!fixture.is_merging());
        assert!(fixture.is_clean());
    }

    #[test]
    fn a_worktree_with_no_origin_default_has_no_upstream() {
        let fixture = MergeFixture::new().without_origin_default();

        let error = fixture.merge().unwrap_err();

        assert_eq!(
            error,
            WorkspaceError::NoUpstream {
                worktree: fixture.worktree.clone()
            }
        );
    }

    #[test]
    fn a_failed_fetch_is_reported() {
        let fixture = MergeFixture::new().without_remote();
        let base = BaseBranch::new(GitTestRepo::BRANCH).unwrap();

        let error = fixture.merge_from(Some(&base)).unwrap_err();

        assert!(
            matches!(&error, WorkspaceError::Failed { subcommand, .. } if subcommand == "fetch"),
            "{error:?}"
        );
    }

    #[test]
    fn a_refused_merge_is_reported_and_leaves_no_merge_in_progress() {
        let fixture = MergeFixture::new();
        fixture.commit_upstream("blocked.txt");
        fs::write(fixture.worktree.join("blocked.txt"), "untracked\n").unwrap();

        let error = fixture.merge().unwrap_err();

        assert!(
            matches!(&error, WorkspaceError::Failed { subcommand, .. } if subcommand == "merge"),
            "{error:?}"
        );
        assert!(!fixture.is_merging());
    }
}
