use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::bounded_command::BoundedCommand;
use super::git_branches::GitBranches;
use super::git_invocation::GitInvocation;
use super::git_porcelain::GitPorcelain;
use super::git_runner::GitRunner;
use super::host_path::HostPath;
use super::start_point_resolver::StartPointResolver;
use super::submodule_commits::SubmoduleCommits;
use crate::application::ports::{
    DirtyState, StartPoint, UnmergedCommits, Workspace, WorkspaceError, WorktreeEntry,
};
use crate::domain::{BaseBranch, DiffStat};

pub struct GitWorkspace {
    git: GitRunner,
}

impl GitWorkspace {
    pub fn new(program: &OsStr, bounded: BoundedCommand) -> Self {
        Self {
            git: GitRunner::new(program, bounded),
        }
    }

    #[cfg(test)]
    pub fn with_environment(
        self,
        overrides: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    ) -> Self {
        Self {
            git: self.git.with_environment(overrides),
        }
    }

    fn worktree_head(
        &self,
        repo: &Path,
        worktree: &Path,
    ) -> Result<Option<String>, WorkspaceError> {
        let canonical = HostPath::new(worktree).canonical_lenient();
        let entries = self.list_worktrees(repo)?;
        let entry = entries.iter().find(|entry| entry.path() == canonical);
        Ok(entry.and_then(WorktreeEntry::head).map(str::to_string))
    }

    fn branch_commits(
        &self,
        repo: &Path,
        branch: &str,
        positives: &[&str],
    ) -> Result<u64, WorkspaceError> {
        if positives.is_empty() {
            return Ok(0);
        }
        let invocation = GitInvocation::in_repo(repo, "rev-list")
            .arg("--count")
            .args(positives)
            .args([
                "--not",
                &format!("--exclude={branch}"),
                "--branches",
                "--tags",
                "--remotes",
            ]);
        Ok(self
            .git
            .run_checked(&invocation)?
            .first_line()
            .parse()
            .unwrap_or(0))
    }

    fn empty_tree(&self, worktree: &Path) -> Result<String, WorkspaceError> {
        let invocation =
            GitInvocation::in_worktree(worktree, "hash-object").args(["-t", "tree", "/dev/null"]);
        Ok(self.git.run_checked(&invocation)?.first_line().to_string())
    }
}

impl Workspace for GitWorkspace {
    fn create_worktree(
        &self,
        repo: &Path,
        worktree: &Path,
        branch: &str,
        start: &StartPoint,
    ) -> Result<(), WorkspaceError> {
        let add = GitInvocation::in_repo(repo, "worktree")
            .writing()
            .arg("add");
        let add = match start {
            StartPoint::Commit(sha) => add
                .args(["--no-track", "-b", branch])
                .arg(worktree)
                .arg(sha),
            StartPoint::Orphan => add.args(["--orphan", "-b", branch]).arg(worktree),
        };
        self.git.run_checked(&add).map(drop)
    }

    fn remove_worktree(&self, repo: &Path, worktree: &Path) -> Result<(), WorkspaceError> {
        let remove = GitInvocation::in_repo(repo, "worktree")
            .writing()
            .args(["remove", "--force"])
            .arg(worktree);
        self.git.run_checked(&remove).map(drop)
    }

    fn unlock_worktree(&self, repo: &Path, worktree: &Path) -> Result<(), WorkspaceError> {
        let unlock = GitInvocation::in_repo(repo, "worktree")
            .writing()
            .arg("unlock")
            .arg(worktree);
        self.git.run_checked(&unlock).map(drop)
    }

    fn list_worktrees(&self, repo: &Path) -> Result<Vec<WorktreeEntry>, WorkspaceError> {
        let list = GitInvocation::in_repo(repo, "worktree").args(["list", "--porcelain", "-z"]);
        Ok(self.git.run_checked(&list)?.worktrees())
    }

    fn list_worktree_dirs(&self, root: &Path) -> Result<Vec<PathBuf>, WorkspaceError> {
        Ok(HostPath::new(root)
            .subdirectories()
            .iter()
            .flat_map(|repo| HostPath::new(repo).subdirectories())
            .collect())
    }

    fn delete_branch(
        &self,
        repo: &Path,
        branch: &str,
        expected_tip: Option<&str>,
    ) -> Result<(), WorkspaceError> {
        GitBranches::new(&self.git).delete(repo, branch, expected_tip)
    }

    fn rename_branch(&self, location: &Path, old: &str, new: &str) -> Result<(), WorkspaceError> {
        GitBranches::new(&self.git).rename(location, old, new)
    }

    fn branch_exists(&self, repo: &Path, branch: &str) -> Result<bool, WorkspaceError> {
        if GitBranches::new(&self.git).has_ref(repo, branch)? {
            return Ok(true);
        }
        let entries = self.list_worktrees(repo)?;
        Ok(entries.iter().any(|entry| entry.branch() == Some(branch)))
    }

    fn first_branch_under(
        &self,
        repo: &Path,
        branch: &str,
    ) -> Result<Option<String>, WorkspaceError> {
        GitBranches::new(&self.git).first_under(repo, branch)
    }

    fn mark_branch_managed(&self, repo: &Path, branch: &str) -> Result<(), WorkspaceError> {
        GitBranches::new(&self.git).mark_managed(repo, branch)
    }

    fn is_branch_managed(&self, repo: &Path, branch: &str) -> Result<bool, WorkspaceError> {
        GitBranches::new(&self.git).is_managed(repo, branch)
    }

    fn list_managed_branches(&self, repo: &Path) -> Result<Vec<String>, WorkspaceError> {
        GitBranches::new(&self.git).list_managed(repo)
    }

    fn is_branch_published(&self, repo: &Path, branch: &str) -> Result<bool, WorkspaceError> {
        GitBranches::new(&self.git).is_published(repo, branch)
    }

    fn resolve_start_point(
        &self,
        repo: &Path,
        base_branch: Option<&BaseBranch>,
    ) -> Result<StartPoint, WorkspaceError> {
        StartPointResolver::new(&self.git).resolve(repo, base_branch)
    }

    fn dirty_state(&self, worktree: &Path) -> Result<DirtyState, WorkspaceError> {
        if !worktree.is_dir() {
            return Ok(DirtyState::missing());
        }
        let status = GitInvocation::in_worktree(worktree, "status").args([
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--no-renames",
            "--ignore-submodules=none",
        ]);
        Ok(self.git.run_checked(&status)?.dirty_state(worktree))
    }

    fn unmerged_commits(
        &self,
        repo: &Path,
        branch: &str,
        worktree: &Path,
    ) -> Result<UnmergedCommits, WorkspaceError> {
        let tip = self.git.resolve(repo, &format!("refs/heads/{branch}"))?;
        let head = self.worktree_head(repo, worktree)?;
        let positives: Vec<&str> = tip.iter().chain(head.iter()).map(String::as_str).collect();
        let branch_count = self.branch_commits(repo, branch, &positives)?;
        let submodule_count = SubmoduleCommits::new(&self.git).count(repo, worktree)?;
        Ok(UnmergedCommits::new(
            branch_count + submodule_count,
            tip.as_deref(),
            head.as_deref(),
        ))
    }

    fn diff_stat(&self, worktree: &Path, base_commit: &str) -> Result<DiffStat, WorkspaceError> {
        let base = match base_commit {
            "" => self.empty_tree(worktree)?,
            commit => commit.to_string(),
        };
        let numstat = GitInvocation::in_worktree(worktree, "diff-index")
            .diffing()
            .args(["--numstat", "--no-renames", &base]);
        let untracked = GitInvocation::in_worktree(worktree, "ls-files")
            .diffing()
            .args(["--others", "--exclude-standard", "-z"]);
        let changes = self.git.run_checked(&numstat)?.file_changes();
        let untracked_count = self.git.run_checked(&untracked)?.record_count();
        Ok(DiffStat::from_changes(changes, untracked_count))
    }

    fn validate_repo(&self, path: &Path) -> Result<PathBuf, WorkspaceError> {
        let check = GitInvocation::in_repo(path, "rev-parse").args([
            "--path-format=absolute",
            "--show-toplevel",
            "--git-dir",
            "--git-common-dir",
            "--show-superproject-working-tree",
        ]);
        let output = self.git.run(&check)?;
        if !output.succeeded() {
            return Err(WorkspaceError::NotARepository {
                path: path.to_path_buf(),
                detail: output.stderr_text(),
            });
        }
        GitPorcelain::new(output.stdout()).top_level(path)
    }

    fn version(&self) -> Result<String, WorkspaceError> {
        let version = GitInvocation::anywhere("--version");
        let line = self.git.run_checked(&version)?.first_line().to_string();
        Ok(line.trim_start_matches("git version ").to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use std::ffi::OsString;
    use std::fs;

    use super::*;
    use crate::adapters::bounded_command::Deadlines;
    use crate::application::ports::BranchMarker;
    use crate::domain::SessionName;
    use crate::test_support::GitTestRepo;

    const BRANCH: &str = "ringleader/fix-auth";
    const RENAMED: &str = "ringleader/fix-login";

    struct WorktreeFixture;

    impl WorktreeFixture {
        fn target(fixture: &GitTestRepo) -> PathBuf {
            fixture.sibling("worktrees").join("fix-auth")
        }

        fn created(fixture: &GitTestRepo) -> PathBuf {
            let workspace = fixture.workspace();
            let target = Self::target(fixture);
            let start = workspace.resolve_start_point(fixture.path(), None).unwrap();
            workspace
                .create_worktree(fixture.path(), &target, BRANCH, &start)
                .unwrap();
            target
        }

        fn marked(fixture: &GitTestRepo) -> PathBuf {
            let worktree = Self::created(fixture);
            fixture
                .workspace()
                .mark_branch_managed(fixture.path(), BRANCH)
                .unwrap();
            worktree
        }

        fn is_managed(fixture: &GitTestRepo, branch: &str) -> bool {
            fixture
                .workspace()
                .is_branch_managed(fixture.path(), branch)
                .unwrap()
        }

        fn unmerged(fixture: &GitTestRepo, worktree: &Path) -> UnmergedCommits {
            fixture
                .workspace()
                .unmerged_commits(fixture.path(), BRANCH, worktree)
                .unwrap()
        }

        fn listed(fixture: &GitTestRepo, worktree: &Path) -> bool {
            fixture
                .workspace()
                .list_worktrees(fixture.path())
                .unwrap()
                .iter()
                .any(|entry| entry.path() == worktree)
        }

        fn clone_of(fixture: &GitTestRepo, name: &str) -> PathBuf {
            let clone = fixture.sibling(name);
            let source = fixture.path().to_string_lossy().into_owned();
            let target = clone.to_string_lossy().into_owned();
            fixture.git(fixture.root(), &["clone", "-q", &source, &target]);
            clone
        }

        fn clone_with_branch(origin: &GitTestRepo) -> PathBuf {
            let clone = Self::clone_of(origin, "clone");
            let workspace = origin.workspace();
            let start = workspace.resolve_start_point(&clone, None).unwrap();
            workspace
                .create_worktree(&clone, &origin.sibling("wt"), BRANCH, &start)
                .unwrap();
            clone
        }

        fn is_published(fixture: &GitTestRepo, repo: &Path) -> bool {
            fixture
                .workspace()
                .is_branch_published(repo, BRANCH)
                .unwrap()
        }

        fn hook(fixture: &GitTestRepo, name: &str, script: &str) {
            let hooks = fixture.path().join(".git/hooks");
            fs::create_dir_all(&hooks).unwrap();
            let hook = hooks.join(name);
            fs::write(&hook, format!("#!/bin/sh\n{script}\n")).unwrap();
            fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        }

        fn roll_back(fixture: &GitTestRepo, worktree: &Path, start: &StartPoint) {
            let workspace = fixture.workspace();
            let entries = workspace.list_worktrees(fixture.path()).unwrap();
            if let Some(entry) = entries.iter().find(|entry| entry.path() == worktree) {
                if entry.is_locked() {
                    workspace.unlock_worktree(fixture.path(), worktree).unwrap();
                }
                workspace.remove_worktree(fixture.path(), worktree).unwrap();
            }
            if workspace.branch_exists(fixture.path(), BRANCH).unwrap() {
                let tip = match start {
                    StartPoint::Commit(sha) => Some(sha.as_str()),
                    StartPoint::Orphan => None,
                };
                workspace
                    .delete_branch(fixture.path(), BRANCH, tip)
                    .unwrap();
            }
        }
    }

    #[test]
    fn a_new_branch_in_a_cloned_repo_has_no_upstream() {
        let origin = GitTestRepo::new();

        let clone = WorktreeFixture::clone_with_branch(&origin);

        let upstream = origin.try_git(
            &clone,
            &[
                "rev-parse",
                "--abbrev-ref",
                &format!("{BRANCH}@{{upstream}}"),
            ],
        );
        assert!(!upstream.status.success());
    }

    #[test]
    fn a_branch_never_pushed_is_not_published() {
        let origin = GitTestRepo::new();
        let clone = WorktreeFixture::clone_with_branch(&origin);

        assert!(!WorktreeFixture::is_published(&origin, &clone));
    }

    #[test]
    fn a_pushed_branch_is_published() {
        let origin = GitTestRepo::new();
        let clone = WorktreeFixture::clone_with_branch(&origin);

        origin.git(&clone, &["push", "-q", "origin", BRANCH]);

        assert!(WorktreeFixture::is_published(&origin, &clone));
    }

    #[test]
    fn a_remote_branch_nested_under_the_branch_name_does_not_publish_it() {
        let origin = GitTestRepo::new();
        let clone = WorktreeFixture::clone_with_branch(&origin);
        let head = origin.git(&clone, &["rev-parse", "HEAD"]);

        let nested = format!("refs/remotes/origin/{BRANCH}/draft");
        origin.git(&clone, &["update-ref", &nested, &head]);

        assert!(!WorktreeFixture::is_published(&origin, &clone));
    }

    #[test]
    fn an_upstream_remote_publishes_the_branch() {
        let fixture = GitTestRepo::new();
        WorktreeFixture::created(&fixture);

        let key = format!("branch.{BRANCH}.remote");
        fixture.git(fixture.path(), &["config", &key, "origin"]);

        assert!(WorktreeFixture::is_published(&fixture, fixture.path()));
    }

    #[test]
    fn the_ladder_prefers_the_local_default_branch_over_origin() {
        let origin = GitTestRepo::new();
        let clone = WorktreeFixture::clone_of(&origin, "clone");
        let local_main = origin.commit(&clone, "unpushed.md");

        let start = origin
            .workspace()
            .resolve_start_point(&clone, None)
            .unwrap();

        assert_eq!(start, StartPoint::Commit(local_main));
    }

    #[test]
    fn the_ladder_keeps_the_local_default_branch_when_origin_is_ahead() {
        let origin = GitTestRepo::new();
        let clone = WorktreeFixture::clone_of(&origin, "clone");
        let local_main = origin.git(&clone, &["rev-parse", "HEAD"]);
        origin.commit(origin.path(), "merged.md");
        origin.git(&clone, &["fetch", "-q", "origin"]);

        let start = origin
            .workspace()
            .resolve_start_point(&clone, None)
            .unwrap();

        assert_eq!(start, StartPoint::Commit(local_main));
    }

    #[test]
    fn the_ladder_names_the_default_branch_after_origin_head() {
        let origin = GitTestRepo::new();
        origin.git(
            origin.path(),
            &["branch", "-m", GitTestRepo::BRANCH, "trunk"],
        );
        let clone = WorktreeFixture::clone_of(&origin, "clone");
        let local_trunk = origin.commit(&clone, "unpushed.md");

        let start = origin
            .workspace()
            .resolve_start_point(&clone, None)
            .unwrap();

        assert_eq!(start, StartPoint::Commit(local_trunk));
    }

    #[test]
    fn the_ladder_falls_back_to_origin_without_a_local_default_branch() {
        let origin = GitTestRepo::new();
        let origin_main = origin.git(origin.path(), &["rev-parse", "HEAD"]);
        let clone = WorktreeFixture::clone_of(&origin, "clone");
        origin.git(&clone, &["checkout", "-q", "-b", "feature"]);
        origin.git(&clone, &["branch", "-q", "-D", GitTestRepo::BRANCH]);
        origin.commit(&clone, "feature.md");

        let start = origin
            .workspace()
            .resolve_start_point(&clone, None)
            .unwrap();

        assert_eq!(start, StartPoint::Commit(origin_main));
    }

    #[test]
    fn the_ladder_tries_the_origin_default_branch_before_local_master() {
        let origin = GitTestRepo::new();
        let clone = WorktreeFixture::clone_of(&origin, "clone");
        origin.git(&clone, &["branch", "-m", GitTestRepo::BRANCH, "master"]);
        origin.git(&clone, &["branch", "--unset-upstream", "master"]);
        let origin_main = origin.commit(origin.path(), "merged.md");
        origin.git(&clone, &["fetch", "-q", "origin"]);

        let start = origin
            .workspace()
            .resolve_start_point(&clone, None)
            .unwrap();

        assert_eq!(start, StartPoint::Commit(origin_main));
    }

    #[test]
    fn the_ladder_skips_an_origin_head_that_names_a_deleted_branch() {
        let origin = GitTestRepo::new();
        origin.git(
            origin.path(),
            &["branch", "-m", GitTestRepo::BRANCH, "master"],
        );
        let clone = WorktreeFixture::clone_of(&origin, "clone");
        origin.git(
            origin.path(),
            &["branch", "-m", "master", GitTestRepo::BRANCH],
        );
        origin.commit(origin.path(), "renamed.md");
        origin.git(&clone, &["fetch", "-q", "--prune", "origin"]);
        origin.git(&clone, &["checkout", "-q", GitTestRepo::BRANCH]);
        let local_main = origin.commit(&clone, "unpushed.md");

        let start = origin
            .workspace()
            .resolve_start_point(&clone, None)
            .unwrap();

        assert_eq!(start, StartPoint::Commit(local_main));
    }

    #[test]
    fn an_unborn_head_resolves_to_an_orphan_worktree() {
        let fixture = GitTestRepo::unborn();
        let workspace = fixture.workspace();

        let start = workspace.resolve_start_point(fixture.path(), None).unwrap();
        let target = WorktreeFixture::target(&fixture);
        workspace
            .create_worktree(fixture.path(), &target, BRANCH, &start)
            .unwrap();

        assert_eq!(start, StartPoint::Orphan);
        assert!(WorktreeFixture::listed(&fixture, &target));
    }

    #[test]
    fn a_repo_without_main_or_master_asks_for_a_base_branch() {
        let fixture = GitTestRepo::new();
        fixture.git(fixture.path(), &["branch", "-m", "main", "trunk"]);

        let error = fixture
            .workspace()
            .resolve_start_point(fixture.path(), None)
            .unwrap_err();

        assert!(matches!(error, WorkspaceError::NoBaseBranch { .. }));
    }

    #[test]
    fn an_explicit_base_resolves_the_local_branch_over_a_tag_of_the_same_name() {
        let fixture = GitTestRepo::new();
        let main = fixture.git(fixture.path(), &["rev-parse", "HEAD"]);
        fixture.git(fixture.path(), &["checkout", "-q", "-b", "side"]);
        fixture.commit(fixture.path(), "side.txt");
        fixture.git(fixture.path(), &["tag", "main"]);
        let base = BaseBranch::new("main").unwrap();

        let start = fixture
            .workspace()
            .resolve_start_point(fixture.path(), Some(&base))
            .unwrap();

        assert_eq!(start, StartPoint::Commit(main));
    }

    #[test]
    fn an_explicit_base_found_only_on_origin_fails_with_a_hint() {
        let origin = GitTestRepo::new();
        origin.git(origin.path(), &["branch", "feature"]);
        let clone = WorktreeFixture::clone_of(&origin, "clone");
        let base = BaseBranch::new("feature").unwrap();

        let error = origin
            .workspace()
            .resolve_start_point(&clone, Some(&base))
            .unwrap_err();

        assert!(error.to_string().contains("did you mean origin/feature?"));
    }

    #[test]
    fn an_explicit_remote_tracking_base_resolves() {
        let origin = GitTestRepo::new();
        let clone = WorktreeFixture::clone_of(&origin, "clone");
        let base = BaseBranch::new("origin/main").unwrap();

        let start = origin.workspace().resolve_start_point(&clone, Some(&base));

        assert!(matches!(start, Ok(StartPoint::Commit(_))));
    }

    #[test]
    fn removal_refuses_the_main_worktree() {
        let fixture = GitTestRepo::new();

        let result = fixture
            .workspace()
            .remove_worktree(fixture.path(), fixture.path());

        assert!(result.is_err());
        assert!(fixture.path().join("README.md").exists());
    }

    #[test]
    fn removal_refuses_a_locked_worktree() {
        let fixture = GitTestRepo::new();
        let worktree = WorktreeFixture::created(&fixture);
        fixture.git(
            fixture.path(),
            &["worktree", "lock", &worktree.to_string_lossy()],
        );

        let result = fixture
            .workspace()
            .remove_worktree(fixture.path(), &worktree);

        assert!(result.is_err());
        assert!(worktree.join("README.md").exists());
    }

    #[test]
    fn removal_refuses_a_plain_directory() {
        let fixture = GitTestRepo::new();
        let plain = fixture.sibling("plain");
        fs::create_dir(&plain).unwrap();
        fs::write(plain.join("keep.txt"), "kept").unwrap();

        let result = fixture.workspace().remove_worktree(fixture.path(), &plain);

        assert!(result.is_err());
        assert!(plain.join("keep.txt").exists());
    }

    #[test]
    fn a_partial_removal_fails_after_unregistering_the_worktree() {
        let fixture = GitTestRepo::new();
        let worktree = WorktreeFixture::created(&fixture);
        let sealed = worktree.join("sealed");
        fs::create_dir(&sealed).unwrap();
        fs::write(sealed.join("file.txt"), "content").unwrap();
        fs::set_permissions(&sealed, fs::Permissions::from_mode(0o555)).unwrap();
        let writable_anyway = fs::write(sealed.join("probe"), "").is_ok();

        let result = fixture
            .workspace()
            .remove_worktree(fixture.path(), &worktree);

        fs::set_permissions(&sealed, fs::Permissions::from_mode(0o755)).unwrap();
        if writable_anyway {
            return;
        }
        assert!(result.is_err());
        assert!(!WorktreeFixture::listed(&fixture, &worktree));
    }

    #[test]
    fn sample_slugs_are_legal_branch_names() {
        let fixture = GitTestRepo::new();
        let samples = [
            "Fix Auth",
            "a:b/c",
            "--x--",
            "café",
            "HEAD",
            &"z".repeat(90),
        ];

        for sample in samples {
            let branch = SessionName::new(sample).unwrap().to_string();
            fixture.git(fixture.path(), &["check-ref-format", "--branch", &branch]);
        }
    }

    #[test]
    fn a_commit_only_on_the_session_branch_is_unmerged() {
        let fixture = GitTestRepo::new();
        let worktree = WorktreeFixture::created(&fixture);
        let tip = fixture.commit(&worktree, "work.txt");

        let unmerged = WorktreeFixture::unmerged(&fixture, &worktree);

        assert_eq!(unmerged.count(), 1);
        assert_eq!(unmerged.tip(), Some(tip.as_str()));
    }

    #[test]
    fn a_commit_on_the_worktree_detached_head_is_unmerged() {
        let fixture = GitTestRepo::new();
        let worktree = WorktreeFixture::created(&fixture);
        fixture.git(&worktree, &["checkout", "-q", "--detach"]);
        fixture.commit(&worktree, "detached.txt");

        assert_eq!(WorktreeFixture::unmerged(&fixture, &worktree).count(), 1);
    }

    #[test]
    fn a_remote_tracking_copy_makes_the_commit_merged() {
        let fixture = GitTestRepo::new();
        let worktree = WorktreeFixture::created(&fixture);
        let tip = fixture.commit(&worktree, "work.txt");
        fixture.git(
            fixture.path(),
            &["update-ref", &format!("refs/remotes/origin/{BRANCH}"), &tip],
        );

        assert_eq!(WorktreeFixture::unmerged(&fixture, &worktree).count(), 0);
    }

    #[test]
    fn another_branch_at_the_tip_makes_the_commit_merged() {
        let fixture = GitTestRepo::new();
        let worktree = WorktreeFixture::created(&fixture);
        let tip = fixture.commit(&worktree, "work.txt");
        fixture.git(fixture.path(), &["branch", "keep", &tip]);

        assert_eq!(WorktreeFixture::unmerged(&fixture, &worktree).count(), 0);
    }

    #[test]
    fn an_orphan_branch_is_unborn_before_its_first_commit() {
        let fixture = GitTestRepo::unborn();
        let worktree = WorktreeFixture::created(&fixture);
        let workspace = fixture.workspace();

        let unmerged = workspace
            .unmerged_commits(fixture.path(), BRANCH, &worktree)
            .unwrap();

        assert!(workspace.branch_exists(fixture.path(), BRANCH).unwrap());
        assert_eq!(
            (unmerged.count(), unmerged.tip(), unmerged.head()),
            (0, None, None)
        );
    }

    #[test]
    fn a_missing_branch_that_no_worktree_names_has_nothing_unmerged() {
        let fixture = GitTestRepo::new();

        let unmerged = WorktreeFixture::unmerged(&fixture, &WorktreeFixture::target(&fixture));

        assert_eq!((unmerged.count(), unmerged.tip()), (0, None));
    }

    #[test]
    fn a_detached_commit_in_a_deleted_worktree_is_still_counted() {
        let fixture = GitTestRepo::new();
        let worktree = WorktreeFixture::created(&fixture);
        fixture.git(&worktree, &["checkout", "-q", "--detach"]);
        fixture.commit(&worktree, "detached.txt");
        fs::remove_dir_all(&worktree).unwrap();

        assert_eq!(WorktreeFixture::unmerged(&fixture, &worktree).count(), 1);
    }

    #[test]
    fn diff_stat_leaves_the_index_untouched_on_a_stat_dirty_tree() {
        let fixture = GitTestRepo::new();
        let worktree = WorktreeFixture::created(&fixture);
        let index = PathBuf::from(fixture.git(&worktree, &["rev-parse", "--git-path", "index"]));
        let index = if index.is_absolute() {
            index
        } else {
            worktree.join(index)
        };
        std::thread::sleep(Duration::from_millis(1100));
        fs::write(
            worktree.join("README.md"),
            fs::read(worktree.join("README.md")).unwrap(),
        )
        .unwrap();
        let before = fs::metadata(&index).unwrap().modified().unwrap();

        fixture
            .workspace()
            .diff_stat(&worktree, FakeBase::head(&fixture, &worktree).as_str())
            .unwrap();

        assert_eq!(fs::metadata(&index).unwrap().modified().unwrap(), before);
    }

    #[test]
    fn diff_stat_counts_changes_since_the_base_and_untracked_files() {
        let fixture = GitTestRepo::new();
        let worktree = WorktreeFixture::created(&fixture);
        let base = FakeBase::head(&fixture, &worktree);
        fixture.commit(&worktree, "committed.txt");
        fs::write(worktree.join("README.md"), "rewritten\nwith two lines\n").unwrap();
        fs::write(worktree.join("scratch.txt"), "untracked").unwrap();

        let diff = fixture.workspace().diff_stat(&worktree, &base).unwrap();

        assert_eq!(
            (
                diff.files(),
                diff.insertions(),
                diff.deletions(),
                diff.untracked()
            ),
            (2, 3, 1, 1)
        );
    }

    #[test]
    fn diff_stat_of_an_orphan_measures_against_the_empty_tree() {
        let fixture = GitTestRepo::unborn();
        let worktree = WorktreeFixture::created(&fixture);
        fixture.commit(&worktree, "first.txt");

        let diff = fixture.workspace().diff_stat(&worktree, "").unwrap();

        assert_eq!((diff.files(), diff.insertions()), (1, 1));
    }

    #[test]
    fn dirty_state_counts_untracked_files_despite_repository_config() {
        let fixture = GitTestRepo::new();
        fixture.git(
            fixture.path(),
            &["config", "status.showUntrackedFiles", "no"],
        );
        let worktree = WorktreeFixture::created(&fixture);
        fs::write(worktree.join("notes.md"), "notes").unwrap();
        fs::create_dir(worktree.join("drafts")).unwrap();
        fs::write(worktree.join("drafts/plan.md"), "plan").unwrap();

        let dirty = fixture.workspace().dirty_state(&worktree).unwrap();

        assert_eq!(dirty.dirty_files(), 2);
    }

    #[test]
    fn dirty_state_returns_an_untracked_nested_repository_as_one_path() {
        let fixture = GitTestRepo::new();
        let worktree = WorktreeFixture::created(&fixture);
        let nested = worktree.join("vendor/tool");
        fs::create_dir_all(&nested).unwrap();
        fixture.git(&nested, &["init", "-q", "-b", "main"]);
        fixture.commit(&nested, "tool.txt");

        let dirty = fixture.workspace().dirty_state(&worktree).unwrap();

        assert_eq!(dirty.nested_repos(), [nested]);
    }

    #[test]
    fn dirty_state_reports_a_missing_worktree_as_missing() {
        let fixture = GitTestRepo::new();

        let dirty = fixture
            .workspace()
            .dirty_state(&WorktreeFixture::target(&fixture))
            .unwrap();

        assert!(dirty.is_missing());
    }

    #[test]
    fn dirty_state_fails_instead_of_reading_an_ancestor_repository() {
        let fixture = GitTestRepo::new();
        let ancestor = fixture.init_repo("ancestor");
        let worktree = ancestor.join("nested/fix-auth");
        let workspace = fixture.workspace();
        let start = workspace.resolve_start_point(fixture.path(), None).unwrap();
        workspace
            .create_worktree(fixture.path(), &worktree, BRANCH, &start)
            .unwrap();
        fs::remove_file(worktree.join(".git")).unwrap();

        let error = workspace.dirty_state(&worktree).unwrap_err();

        assert!(error.to_string().contains("not a git repository"));
    }

    #[test]
    fn reads_never_run_the_repository_fsmonitor_hook() {
        let fixture = GitTestRepo::new();
        let marker = fixture.sibling("fsmonitor-ran");
        let hook = fixture.sibling("fsmonitor-hook");
        fs::write(
            &hook,
            format!("#!/bin/sh\necho ran >> '{}'\n", marker.display()),
        )
        .unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        fixture.git(
            fixture.path(),
            &["config", "core.fsmonitor", &hook.to_string_lossy()],
        );
        let worktree = WorktreeFixture::created(&fixture);
        let base = FakeBase::head(&fixture, &worktree);
        let _ = fs::remove_file(&marker);

        fixture.workspace().dirty_state(&worktree).unwrap();
        fixture.workspace().diff_stat(&worktree, &base).unwrap();

        assert!(!marker.exists());
    }

    #[test]
    fn a_stale_expected_tip_keeps_the_branch() {
        let fixture = GitTestRepo::new();
        let worktree = WorktreeFixture::created(&fixture);
        let stale = fixture.git(fixture.path(), &["rev-parse", "HEAD"]);
        fixture.commit(&worktree, "work.txt");
        fixture
            .workspace()
            .remove_worktree(fixture.path(), &worktree)
            .unwrap();

        let result = fixture
            .workspace()
            .delete_branch(fixture.path(), BRANCH, Some(&stale));

        assert!(result.is_err());
        assert!(
            fixture
                .workspace()
                .branch_exists(fixture.path(), BRANCH)
                .unwrap()
        );
    }

    #[test]
    fn a_branch_deletion_leaves_no_config_section() {
        let fixture = GitTestRepo::new();
        let worktree = WorktreeFixture::created(&fixture);
        let tip = fixture.git(&worktree, &["rev-parse", "HEAD"]);
        fixture.git(
            fixture.path(),
            &["config", &format!("branch.{BRANCH}.remote"), "origin"],
        );
        fixture
            .workspace()
            .remove_worktree(fixture.path(), &worktree)
            .unwrap();

        fixture
            .workspace()
            .delete_branch(fixture.path(), BRANCH, Some(&tip))
            .unwrap();

        let section = fixture.try_git(
            fixture.path(),
            &["config", "--get-regexp", "^branch\\.ringleader/fix-auth\\."],
        );
        assert!(!section.status.success());
        assert!(
            !fixture
                .workspace()
                .branch_exists(fixture.path(), BRANCH)
                .unwrap()
        );
    }

    #[test]
    fn a_deletion_without_a_tip_removes_only_a_leftover_section() {
        let fixture = GitTestRepo::new();
        fixture.git(
            fixture.path(),
            &["config", &format!("branch.{BRANCH}.merge"), "refs/heads/x"],
        );

        fixture
            .workspace()
            .delete_branch(fixture.path(), BRANCH, None)
            .unwrap();

        let section = fixture.try_git(
            fixture.path(),
            &["config", "--get", &format!("branch.{BRANCH}.merge")],
        );
        assert!(!section.status.success());
    }

    #[test]
    fn git_dir_and_work_tree_variables_never_redirect_the_calls() {
        let fixture = GitTestRepo::new();
        let other = fixture.init_repo("other");
        let mut environment = fixture.environment();
        environment.push(("GIT_DIR".into(), other.join(".git").into_os_string()));
        environment.push(("GIT_WORK_TREE".into(), other.clone().into_os_string()));
        let workspace = GitWorkspace::new(
            &OsString::from("git"),
            BoundedCommand::new(Deadlines::standard()),
        )
        .with_environment(environment);
        let target = WorktreeFixture::target(&fixture);
        let start = workspace.resolve_start_point(fixture.path(), None).unwrap();

        let top_level = workspace.validate_repo(fixture.path()).unwrap();
        workspace
            .create_worktree(fixture.path(), &target, BRANCH, &start)
            .unwrap();

        assert_eq!(top_level, fixture.path());
        assert!(WorktreeFixture::listed(&fixture, &target));
    }

    #[test]
    fn a_failing_post_checkout_hook_leaves_nothing_after_rollback() {
        let fixture = GitTestRepo::new();
        WorktreeFixture::hook(&fixture, "post-checkout", "exit 1");
        let workspace = fixture.workspace();
        let start = workspace.resolve_start_point(fixture.path(), None).unwrap();
        let target = WorktreeFixture::target(&fixture);

        let result = workspace.create_worktree(fixture.path(), &target, BRANCH, &start);
        WorktreeFixture::roll_back(&fixture, &target, &start);

        assert!(result.is_err());
        assert!(!WorktreeFixture::listed(&fixture, &target));
        assert!(!workspace.branch_exists(fixture.path(), BRANCH).unwrap());
    }

    #[test]
    fn a_hook_that_locks_the_new_worktree_leaves_nothing_after_rollback() {
        let fixture = GitTestRepo::new();
        WorktreeFixture::hook(
            &fixture,
            "post-checkout",
            "git worktree lock \"$PWD\"\nexit 1",
        );
        let workspace = fixture.workspace();
        let start = workspace.resolve_start_point(fixture.path(), None).unwrap();
        let target = WorktreeFixture::target(&fixture);

        let result = workspace.create_worktree(fixture.path(), &target, BRANCH, &start);
        WorktreeFixture::roll_back(&fixture, &target, &start);

        assert!(result.is_err());
        assert!(!WorktreeFixture::listed(&fixture, &target));
        assert!(!workspace.branch_exists(fixture.path(), BRANCH).unwrap());
    }

    #[test]
    fn a_smudge_filter_past_the_deadline_times_out_and_leaves_no_worktree() {
        let fixture = GitTestRepo::new();
        fs::write(fixture.path().join(".gitattributes"), "*.txt filter=slow\n").unwrap();
        fixture.git(
            fixture.path(),
            &["config", "filter.slow.smudge", "sleep 5; cat"],
        );
        fixture.git(fixture.path(), &["config", "filter.slow.clean", "cat"]);
        fixture.git(fixture.path(), &["add", ".gitattributes"]);
        fixture.commit(fixture.path(), "slow.txt");
        let workspace = fixture.workspace_with_deadline(Duration::from_secs(1));
        let start = fixture
            .workspace()
            .resolve_start_point(fixture.path(), None)
            .unwrap();
        let target = WorktreeFixture::target(&fixture);

        let error = workspace
            .create_worktree(fixture.path(), &target, BRANCH, &start)
            .unwrap_err();

        assert!(matches!(error, WorkspaceError::TimedOut { .. }));
        assert!(!WorktreeFixture::listed(&fixture, &target));
    }

    #[test]
    fn validation_accepts_a_plain_repository() {
        let fixture = GitTestRepo::new();

        assert_eq!(
            fixture.workspace().validate_repo(fixture.path()).unwrap(),
            fixture.path()
        );
    }

    #[test]
    fn validation_resolves_a_subdirectory_to_the_top_level() {
        let fixture = GitTestRepo::new();
        let subdirectory = fixture.path().join("src");
        fs::create_dir(&subdirectory).unwrap();

        assert_eq!(
            fixture.workspace().validate_repo(&subdirectory).unwrap(),
            fixture.path()
        );
    }

    #[test]
    fn validation_rejects_a_linked_worktree() {
        let fixture = GitTestRepo::new();
        let worktree = WorktreeFixture::created(&fixture);

        let error = fixture.workspace().validate_repo(&worktree).unwrap_err();

        assert!(matches!(error, WorkspaceError::LinkedWorktree { .. }));
    }

    #[test]
    fn validation_rejects_a_submodule() {
        let fixture = GitTestRepo::new();
        let library = fixture.init_repo("library");
        fixture.add_submodule(fixture.path(), &library, "vendor/library");

        let error = fixture
            .workspace()
            .validate_repo(&fixture.path().join("vendor/library"))
            .unwrap_err();

        assert!(matches!(error, WorkspaceError::Submodule { .. }));
    }

    #[test]
    fn validation_rejects_a_plain_directory() {
        let fixture = GitTestRepo::new();
        let plain = fixture.sibling("plain");
        fs::create_dir(&plain).unwrap();

        let error = fixture.workspace().validate_repo(&plain).unwrap_err();

        assert!(matches!(error, WorkspaceError::NotARepository { .. }));
    }

    #[test]
    fn rename_branch_renames_inside_the_worktree() {
        let fixture = GitTestRepo::new();
        let worktree = WorktreeFixture::created(&fixture);

        fixture
            .workspace()
            .rename_branch(&worktree, BRANCH, RENAMED)
            .unwrap();

        assert_eq!(
            fixture.git(&worktree, &["branch", "--show-current"]),
            RENAMED
        );
    }

    #[test]
    fn marking_a_branch_sets_its_config_entry() {
        let fixture = GitTestRepo::new();

        WorktreeFixture::marked(&fixture);

        let key = format!("branch.{BRANCH}.ringleaderManaged");
        assert_eq!(
            fixture.git(fixture.path(), &["config", "--get", &key]),
            "true"
        );
    }

    #[test]
    fn the_printed_marking_command_marks_the_branch_through_a_shell() {
        let fixture = GitTestRepo::new();
        let repo = fixture.init_repo("example's project");
        let branch = "feat(ui)";
        fixture.git(&repo, &["branch", branch]);
        let command = BranchMarker::new(branch).marking_command(&repo);

        fixture.shell(&command);

        assert!(
            fixture
                .workspace()
                .is_branch_managed(&repo, branch)
                .unwrap()
        );
    }

    #[test]
    fn a_marked_branch_is_managed() {
        let fixture = GitTestRepo::new();

        WorktreeFixture::marked(&fixture);

        assert!(WorktreeFixture::is_managed(&fixture, BRANCH));
    }

    #[test]
    fn an_unmarked_branch_is_not_managed() {
        let fixture = GitTestRepo::new();
        WorktreeFixture::created(&fixture);

        assert!(!WorktreeFixture::is_managed(&fixture, BRANCH));
    }

    #[test]
    fn a_marker_with_another_value_is_not_managed() {
        let fixture = GitTestRepo::new();
        WorktreeFixture::created(&fixture);
        let key = format!("branch.{BRANCH}.ringleaderManaged");
        fixture.git(fixture.path(), &["config", &key, "false"]);

        assert!(!WorktreeFixture::is_managed(&fixture, BRANCH));
    }

    #[test]
    fn deleting_a_marked_branch_removes_its_marker() {
        let fixture = GitTestRepo::new();
        let worktree = WorktreeFixture::marked(&fixture);
        let tip = fixture.git(&worktree, &["rev-parse", "HEAD"]);
        let workspace = fixture.workspace();
        workspace
            .remove_worktree(fixture.path(), &worktree)
            .unwrap();

        workspace
            .delete_branch(fixture.path(), BRANCH, Some(&tip))
            .unwrap();

        assert!(!WorktreeFixture::is_managed(&fixture, BRANCH));
    }

    #[test]
    fn renaming_a_marked_branch_carries_its_marker() {
        let fixture = GitTestRepo::new();
        let worktree = WorktreeFixture::marked(&fixture);

        fixture
            .workspace()
            .rename_branch(&worktree, BRANCH, RENAMED)
            .unwrap();

        assert!(WorktreeFixture::is_managed(&fixture, RENAMED));
        assert!(!WorktreeFixture::is_managed(&fixture, BRANCH));
    }

    #[test]
    fn renaming_a_marked_unborn_branch_carries_its_marker() {
        let fixture = GitTestRepo::unborn();
        let worktree = WorktreeFixture::marked(&fixture);

        fixture
            .workspace()
            .rename_branch(&worktree, BRANCH, RENAMED)
            .unwrap();

        assert!(WorktreeFixture::is_managed(&fixture, RENAMED));
        assert!(!WorktreeFixture::is_managed(&fixture, BRANCH));
    }

    #[test]
    fn deleting_a_marked_orphan_without_a_tip_removes_its_marker() {
        let fixture = GitTestRepo::unborn();
        let worktree = WorktreeFixture::marked(&fixture);
        let workspace = fixture.workspace();
        workspace
            .remove_worktree(fixture.path(), &worktree)
            .unwrap();

        workspace
            .delete_branch(fixture.path(), BRANCH, None)
            .unwrap();

        assert!(!WorktreeFixture::is_managed(&fixture, BRANCH));
    }

    #[test]
    fn listing_managed_branches_lists_marked_branches_that_exist() {
        let fixture = GitTestRepo::new();
        WorktreeFixture::marked(&fixture);
        let workspace = fixture.workspace();
        fixture.git(fixture.path(), &["branch", "feature"]);
        fixture.git(fixture.path(), &["branch", "Feature/v1.2"]);
        workspace
            .mark_branch_managed(fixture.path(), "Feature/v1.2")
            .unwrap();
        workspace
            .mark_branch_managed(fixture.path(), "gone")
            .unwrap();

        let mut branches = workspace.list_managed_branches(fixture.path()).unwrap();

        branches.sort();
        assert_eq!(branches, ["Feature/v1.2", BRANCH]);
    }

    #[test]
    fn listing_managed_branches_skips_a_marker_with_another_value() {
        let fixture = GitTestRepo::new();
        WorktreeFixture::created(&fixture);
        let key = format!("branch.{BRANCH}.ringleaderManaged");
        fixture.git(fixture.path(), &["config", &key, "false"]);

        let branches = fixture
            .workspace()
            .list_managed_branches(fixture.path())
            .unwrap();

        assert!(branches.is_empty());
    }

    #[test]
    fn listing_managed_branches_without_markers_is_empty() {
        let fixture = GitTestRepo::new();
        WorktreeFixture::created(&fixture);

        let branches = fixture
            .workspace()
            .list_managed_branches(fixture.path())
            .unwrap();

        assert!(branches.is_empty());
    }

    #[test]
    fn the_first_branch_under_a_name_is_found() {
        let fixture = GitTestRepo::new();
        fixture.git(fixture.path(), &["branch", "feature/login"]);

        let nested = fixture
            .workspace()
            .first_branch_under(fixture.path(), "feature")
            .unwrap();

        assert_eq!(nested.as_deref(), Some("feature/login"));
    }

    #[test]
    fn a_branch_sharing_only_the_start_of_a_name_is_not_under_it() {
        let fixture = GitTestRepo::new();
        fixture.git(fixture.path(), &["branch", "feature-login"]);

        let nested = fixture
            .workspace()
            .first_branch_under(fixture.path(), "feature")
            .unwrap();

        assert_eq!(nested, None);
    }

    #[test]
    fn list_worktree_dirs_lists_slug_directories_under_each_repo() {
        let fixture = GitTestRepo::new();
        let root = fixture.sibling("root");
        fs::create_dir_all(root.join("example-project/fix-auth")).unwrap();
        fs::create_dir_all(root.join("example-project/dark-mode")).unwrap();

        let directories = fixture.workspace().list_worktree_dirs(&root).unwrap();

        assert_eq!(
            directories,
            [
                root.join("example-project/dark-mode"),
                root.join("example-project/fix-auth")
            ]
        );
    }

    #[test]
    fn unlock_worktree_unlocks_a_locked_worktree() {
        let fixture = GitTestRepo::new();
        let worktree = WorktreeFixture::created(&fixture);
        fixture.git(
            fixture.path(),
            &["worktree", "lock", &worktree.to_string_lossy()],
        );

        fixture
            .workspace()
            .unlock_worktree(fixture.path(), &worktree)
            .unwrap();

        let entries = fixture.workspace().list_worktrees(fixture.path()).unwrap();
        assert!(entries.iter().all(|entry| !entry.is_locked()));
    }

    #[test]
    fn version_reads_the_git_release() {
        let fixture = GitTestRepo::new();

        let version = fixture.workspace().version().unwrap();

        assert!(version.starts_with("2."));
    }

    struct FakeBase;

    impl FakeBase {
        fn head(fixture: &GitTestRepo, worktree: &Path) -> String {
            fixture.git(worktree, &["rev-parse", "HEAD"])
        }
    }
}
