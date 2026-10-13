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
    const HEAD: &'static str = "HEAD";

    pub fn new(git: &'a GitRunner) -> Self {
        Self { git }
    }

    pub fn fetch(
        &self,
        worktree: &Path,
        base_branch: Option<&BaseBranch>,
    ) -> Result<BaseBranch, WorkspaceError> {
        let branch = self.upstream_branch(worktree, base_branch)?;
        self.fetch_branch(worktree, &branch)?;
        Ok(branch)
    }

    pub fn merge(
        &self,
        worktree: &Path,
        branch: &BaseBranch,
    ) -> Result<UpstreamMerge, WorkspaceError> {
        self.refuse_merge_in_progress(worktree)?;
        self.refuse_staged_changes(worktree)?;
        let commit = self.commit_of(worktree, &branch.origin_ref())?;
        let outcome = self.merge_unless_contained(worktree, branch, &commit)?;
        Ok(UpstreamMerge::new(&branch.origin_suggestion(), outcome))
    }

    fn refuse_merge_in_progress(&self, worktree: &Path) -> Result<(), WorkspaceError> {
        if self.is_merging(worktree)? {
            return Err(WorkspaceError::MergeInProgress {
                worktree: worktree.to_path_buf(),
            });
        }
        Ok(())
    }

    fn refuse_staged_changes(&self, worktree: &Path) -> Result<(), WorkspaceError> {
        if self.has_staged_changes(worktree)? {
            return Err(WorkspaceError::StagedChanges {
                worktree: worktree.to_path_buf(),
            });
        }
        Ok(())
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

    fn fetch_branch(&self, worktree: &Path, branch: &BaseBranch) -> Result<(), WorkspaceError> {
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
        branch: &BaseBranch,
        commit: &str,
    ) -> Result<MergeOutcome, WorkspaceError> {
        if self.head_contains(worktree, commit)? {
            return Ok(MergeOutcome::UpToDate);
        }
        let message = self.merge_message(worktree, branch)?;
        let merge = GitInvocation::in_worktree(worktree, "merge")
            .writing()
            .args([
                "--no-edit",
                "--no-rerere-autoupdate",
                "--no-overwrite-ignore",
                "-m",
                &message,
                &branch.origin_ref(),
            ]);
        match self.git.run_checked(&merge) {
            Ok(_) => Ok(MergeOutcome::Merged),
            Err(failure) => self.conflicts_or(worktree, failure),
        }
    }

    fn merge_message(
        &self,
        worktree: &Path,
        branch: &BaseBranch,
    ) -> Result<String, WorkspaceError> {
        let message = format!(
            "Merge remote-tracking branch '{}'",
            branch.origin_suggestion()
        );
        let checked_out = GitInvocation::in_worktree(worktree, "symbolic-ref").args([
            "--short",
            "-q",
            Self::HEAD,
        ]);
        Ok(match self.git.resolved_from(&checked_out)? {
            Some(current) => format!("{message} into {current}"),
            None => message,
        })
    }

    fn head_contains(&self, worktree: &Path, commit: &str) -> Result<bool, WorkspaceError> {
        let ancestry = GitInvocation::in_worktree(worktree, "merge-base").args([
            "--is-ancestor",
            commit,
            Self::HEAD,
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
        let undo = if self.is_merging(worktree)? {
            GitInvocation::in_worktree(worktree, "merge").arg("--abort")
        } else if self.has_staged_changes(worktree)? {
            GitInvocation::in_worktree(worktree, "reset").args(["--merge", Self::HEAD])
        } else {
            return Ok(failure);
        };
        self.git.run_checked(&undo.writing())?;
        Ok(WorkspaceError::MergeAborted {
            cause: failure.to_string(),
        })
    }

    fn has_staged_changes(&self, worktree: &Path) -> Result<bool, WorkspaceError> {
        let compare = GitInvocation::in_worktree(worktree, "diff-index").args([
            "--cached",
            "--quiet",
            Self::HEAD,
            "--",
        ]);
        Ok(self.git.lookup(&compare)?.is_none())
    }

    fn is_merging(&self, worktree: &Path) -> Result<bool, WorkspaceError> {
        Ok(self.git.resolve(worktree, Self::MERGE_HEAD)?.is_some())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;
    use crate::adapters::GitWorkspace;
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
        const RESOLUTION: &'static str = "both sides kept\n";
        const SHORT_DEADLINE: Duration = Duration::from_secs(1);
        const IGNORED_FILE: &'static str = "settings.local";
        const LOCAL_TEXT: &'static str = "mine\n";

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
            Self::fetch_and_merge(&self.origin.workspace(), &self.worktree, base)
        }

        fn fetch_and_merge(
            workspace: &GitWorkspace,
            worktree: &Path,
            base: Option<&BaseBranch>,
        ) -> Result<UpstreamMerge, WorkspaceError> {
            let branch = workspace.fetch_upstream(worktree, base)?;
            workspace.merge_upstream(worktree, &branch)
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

        fn head_subject(&self) -> String {
            self.origin
                .git(&self.worktree, &["log", "-1", "--format=%s"])
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
            self.install_hook("commit-msg", "echo 'needs a ticket id' >&2\nexit 1");
        }

        fn stall_merge_commits(&self) {
            self.install_hook("pre-merge-commit", "sleep 20");
        }

        fn install_hook(&self, name: &str, body: &str) {
            let hook = self.clone.join(".git/hooks").join(name);
            fs::write(&hook, format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(&hook, fs::Permissions::from_mode(Self::EXECUTABLE)).unwrap();
        }

        fn merge_within(&self, deadline: Duration) -> Result<UpstreamMerge, WorkspaceError> {
            let workspace = self.origin.workspace_with_deadline(deadline);
            Self::fetch_and_merge(&workspace, &self.worktree, None)
        }

        fn stage_a_change(&self) {
            fs::write(self.worktree.join("staged.txt"), "staged\n").unwrap();
            self.origin.git(&self.worktree, &["add", "staged.txt"]);
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

        fn with_rerere_auto_update(self) -> Self {
            self.origin
                .git(&self.clone, &["config", "rerere.enabled", "true"]);
            self.origin
                .git(&self.clone, &["config", "rerere.autoUpdate", "true"]);
            self
        }

        fn record_a_resolution_and_undo_the_merge(&self) {
            fs::write(self.worktree.join(Self::SHARED_FILE), Self::RESOLUTION).unwrap();
            self.origin.git(&self.worktree, &["add", Self::SHARED_FILE]);
            self.origin
                .git(&self.worktree, &["commit", "-q", "--no-edit"]);
            self.origin
                .git(&self.worktree, &["reset", "-q", "--hard", "HEAD~1"]);
        }

        fn shared_text(&self) -> String {
            fs::read_to_string(self.worktree.join(Self::SHARED_FILE)).unwrap()
        }

        fn keep_an_ignored_local_file(&self) {
            let exclude = self.clone.join(".git/info/exclude");
            fs::write(&exclude, format!("{}\n", Self::IGNORED_FILE)).unwrap();
            fs::write(self.worktree.join(Self::IGNORED_FILE), Self::LOCAL_TEXT).unwrap();
        }

        fn ignored_text(&self) -> String {
            fs::read_to_string(self.worktree.join(Self::IGNORED_FILE)).unwrap()
        }

        fn keep_our_side_without_committing(&self) {
            self.origin
                .git(&self.worktree, &["checkout", "--ours", Self::SHARED_FILE]);
            self.origin.git(&self.worktree, &["add", Self::SHARED_FILE]);
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
    fn the_merge_commit_names_origin_the_way_git_does() {
        let fixture = MergeFixture::new();
        fixture.commit_on_branch("work.txt");
        fixture.commit_upstream("news.txt");

        fixture.merge().unwrap();

        assert_eq!(
            fixture.head_subject(),
            "Merge remote-tracking branch 'origin/main' into fix-auth"
        );
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
    fn a_resolution_rerere_replays_is_still_handed_over_as_a_conflict() {
        let fixture = MergeFixture::new().with_rerere_auto_update();
        fixture.conflicting_edits();
        fixture.merge().unwrap();
        fixture.record_a_resolution_and_undo_the_merge();

        let merge = fixture.merge().unwrap();

        assert_eq!(
            MergeFixture::conflicted_paths(&merge),
            [MergeFixture::SHARED_FILE]
        );
        assert_eq!(fixture.shared_text(), MergeFixture::RESOLUTION);
    }

    #[test]
    fn a_fast_forward_keeps_an_ignored_local_file_upstream_starts_tracking() {
        let fixture = MergeFixture::new();
        fixture.keep_an_ignored_local_file();
        fixture.commit_upstream(MergeFixture::IGNORED_FILE);

        let error = fixture.merge().unwrap_err();

        assert!(
            matches!(&error, WorkspaceError::Failed { stderr, .. } if stderr.contains("would be overwritten by merge")),
            "{error:?}"
        );
        assert_eq!(fixture.ignored_text(), MergeFixture::LOCAL_TEXT);
    }

    #[test]
    fn a_merge_killed_before_it_wrote_merge_head_is_undone() {
        let fixture = MergeFixture::new();
        fixture.commit_on_branch("work.txt");
        fixture.commit_upstream("news.txt");
        fixture.stall_merge_commits();

        let error = fixture
            .merge_within(MergeFixture::SHORT_DEADLINE)
            .unwrap_err();

        assert!(
            matches!(error, WorkspaceError::MergeAborted { .. }),
            "{error:?}"
        );
        assert!(fixture.is_clean());
    }

    #[test]
    fn staged_changes_are_refused_and_kept() {
        let fixture = MergeFixture::new();
        fixture.commit_upstream("news.txt");
        fixture.stage_a_change();

        let error = fixture.merge().unwrap_err();

        assert_eq!(
            error,
            WorkspaceError::StagedChanges {
                worktree: fixture.worktree.clone()
            }
        );
        assert!(!fixture.is_clean());
    }

    #[test]
    fn a_merge_already_in_progress_is_refused_and_kept() {
        let fixture = MergeFixture::new();
        fixture.conflicting_edits();
        fixture.merge().unwrap();
        fixture.keep_our_side_without_committing();

        let error = fixture.merge().unwrap_err();

        assert_eq!(
            error,
            WorkspaceError::MergeInProgress {
                worktree: fixture.worktree.clone()
            }
        );
        assert!(fixture.is_merging());
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
            matches!(&error, WorkspaceError::Failed { stderr, .. } if stderr.contains("would be overwritten by merge")),
            "{error:?}"
        );
        assert!(!fixture.is_merging());
    }
}
