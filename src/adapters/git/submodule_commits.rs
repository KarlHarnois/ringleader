use std::fs;
use std::path::{Path, PathBuf};

use super::git_invocation::GitInvocation;
use super::git_runner::GitRunner;
use crate::adapters::filesystem::host_path::HostPath;
use crate::application::ports::WorkspaceError;

pub struct SubmoduleCommits<'a> {
    git: &'a GitRunner,
}

impl<'a> SubmoduleCommits<'a> {
    pub fn new(git: &'a GitRunner) -> Self {
        Self { git }
    }

    pub fn count(&self, repo: &Path, worktree: &Path) -> Result<u64, WorkspaceError> {
        let mut total = 0;
        if let Some(git_dir) = self.worktree_git_dir(repo, worktree)? {
            total += self.count_modules(&git_dir.join("modules"))?;
        }
        if worktree.is_dir() {
            total += self.count_embedded_clones(worktree)?;
        }
        Ok(total)
    }

    fn worktree_git_dir(
        &self,
        repo: &Path,
        worktree: &Path,
    ) -> Result<Option<PathBuf>, WorkspaceError> {
        if worktree.is_dir() {
            let git_dir =
                GitInvocation::in_worktree(worktree, "rev-parse").arg("--absolute-git-dir");
            let listing = self.git.run_checked(&git_dir)?;
            return Ok(Some(PathBuf::from(listing.first_line())));
        }
        self.admin_dir_of_missing_worktree(repo, worktree)
    }

    fn admin_dir_of_missing_worktree(
        &self,
        repo: &Path,
        worktree: &Path,
    ) -> Result<Option<PathBuf>, WorkspaceError> {
        let common_dir = GitInvocation::in_repo(repo, "rev-parse")
            .args(["--path-format=absolute", "--git-common-dir"]);
        let common_dir = PathBuf::from(self.git.run_checked(&common_dir)?.first_line());
        let expected = HostPath::new(&worktree.join(".git")).canonical_lenient();
        Ok(HostPath::new(&common_dir.join("worktrees"))
            .subdirectories()
            .into_iter()
            .find(|admin| Self::names_git_file(admin, &expected)))
    }

    fn names_git_file(admin: &Path, expected: &Path) -> bool {
        let Ok(recorded) = fs::read_to_string(admin.join("gitdir")) else {
            return false;
        };
        let named = admin.join(recorded.trim());
        HostPath::new(&named).canonical_lenient() == expected
    }

    fn count_modules(&self, directory: &Path) -> Result<u64, WorkspaceError> {
        let mut total = 0;
        for child in HostPath::new(directory).subdirectories() {
            if Self::is_git_dir(&child) {
                total += self.count_repository(&child, &child)?;
                total += self.count_modules(&child.join("modules"))?;
            } else {
                total += self.count_modules(&child)?;
            }
        }
        Ok(total)
    }

    fn count_embedded_clones(&self, worktree: &Path) -> Result<u64, WorkspaceError> {
        let staged = GitInvocation::in_worktree(worktree, "ls-files").args(["--stage", "-z"]);
        let listing = self.git.run_checked(&staged)?;
        let mut total = 0;
        for gitlink in listing.gitlink_paths() {
            let clone = worktree.join(gitlink);
            let git_dir = clone.join(".git");
            if git_dir.is_dir() {
                total += self.count_repository(&git_dir, &clone)?;
            }
        }
        Ok(total)
    }

    fn count_repository(&self, git_dir: &Path, work_tree: &Path) -> Result<u64, WorkspaceError> {
        let head = GitInvocation::in_git_dir(git_dir, work_tree, "rev-parse")
            .args(["--verify", "-q", "HEAD"]);
        let mut positives = Vec::new();
        if self.git.resolved_from(&head)?.is_some() {
            positives.push("HEAD");
        }
        let count = GitInvocation::in_git_dir(git_dir, work_tree, "rev-list")
            .arg("--count")
            .args(positives)
            .args(["--branches", "--not", "--remotes"]);
        Ok(self
            .git
            .run_checked(&count)?
            .first_line()
            .parse()
            .unwrap_or(0))
    }

    fn is_git_dir(directory: &Path) -> bool {
        directory.join("HEAD").is_file() && directory.join("objects").is_dir()
    }
}

#[cfg(test)]
mod tests {
    use crate::application::ports::Workspace;
    use crate::test_support::GitTestRepo;

    use std::fs;
    use std::path::{Path, PathBuf};

    const BRANCH: &str = "ringleader/fix-auth";

    struct SubmoduleFixture {
        repo: GitTestRepo,
        worktree: PathBuf,
    }

    impl SubmoduleFixture {
        fn with_library() -> Self {
            let repo = GitTestRepo::new();
            let library = repo.init_repo("library");
            repo.add_submodule(repo.path(), &library, "vendor/library");
            let worktree = repo.add_worktree("fix-auth", BRANCH);
            repo.git(
                &worktree,
                &[
                    "-c",
                    "protocol.file.allow=always",
                    "submodule",
                    "update",
                    "--init",
                    "-q",
                ],
            );
            Self { repo, worktree }
        }

        fn library_checkout(&self) -> PathBuf {
            self.worktree.join("vendor/library")
        }

        fn commit_in_library(&self) -> String {
            self.repo
                .commit(&self.library_checkout(), "local-change.txt")
        }

        fn unmerged_count(&self) -> u64 {
            self.repo
                .workspace()
                .unmerged_commits(self.repo.path(), BRANCH, &self.worktree)
                .unwrap()
                .count()
        }

        fn git_in_worktree(&self, args: &[&str]) -> String {
            self.repo.git(&self.worktree, args)
        }

        fn keep_session_branch_elsewhere(&self) {
            self.repo.git(self.repo.path(), &["branch", "keep", BRANCH]);
        }
    }

    #[test]
    fn a_submodule_commit_counts_after_the_recording_branch_is_merged_elsewhere() {
        let fixture = SubmoduleFixture::with_library();
        fixture.commit_in_library();
        fixture.git_in_worktree(&["add", "vendor/library"]);
        fixture.git_in_worktree(&["commit", "-q", "-m", "Record the library change"]);
        fixture.keep_session_branch_elsewhere();

        assert_eq!(fixture.unmerged_count(), 1);
    }

    #[test]
    fn a_commit_on_a_local_submodule_branch_counts() {
        let fixture = SubmoduleFixture::with_library();
        let library = fixture.library_checkout();
        let detached = fixture.repo.git(&library, &["rev-parse", "HEAD"]);
        fixture
            .repo
            .git(&library, &["checkout", "-q", "-b", "local-work"]);
        fixture.commit_in_library();
        fixture.repo.git(&library, &["checkout", "-q", &detached]);

        assert_eq!(fixture.unmerged_count(), 1);
    }

    #[test]
    fn a_commit_left_behind_by_deinit_counts() {
        let fixture = SubmoduleFixture::with_library();
        fixture.commit_in_library();
        fixture.git_in_worktree(&["submodule", "deinit", "-q", "-f", "vendor/library"]);

        assert_eq!(fixture.unmerged_count(), 1);
    }

    #[test]
    fn a_commit_in_a_submodule_removed_with_git_rm_counts() {
        let fixture = SubmoduleFixture::with_library();
        fixture.commit_in_library();
        fixture.git_in_worktree(&["rm", "-q", "-f", "vendor/library"]);

        assert_eq!(fixture.unmerged_count(), 1);
    }

    #[test]
    fn a_submodule_commit_in_a_deleted_worktree_counts() {
        let fixture = SubmoduleFixture::with_library();
        fixture.commit_in_library();
        fs::remove_dir_all(&fixture.worktree).unwrap();

        assert_eq!(fixture.unmerged_count(), 1);
    }

    #[test]
    fn a_commit_in_a_clone_committed_as_a_gitlink_counts() {
        let fixture = SubmoduleFixture::with_library();
        let source = fixture
            .repo
            .sibling("library")
            .to_string_lossy()
            .into_owned();
        fixture.git_in_worktree(&["clone", "-q", &source, "embedded"]);
        fixture
            .repo
            .commit(&fixture.worktree.join("embedded"), "embedded-change.txt");
        fixture.git_in_worktree(&["add", "embedded"]);
        fixture.git_in_worktree(&["commit", "-q", "-m", "Commit the clone as a gitlink"]);
        fixture.keep_session_branch_elsewhere();

        assert!(Path::new(&fixture.worktree.join("embedded/.git")).is_dir());
        assert_eq!(fixture.unmerged_count(), 1);
    }

    #[test]
    fn a_pushed_submodule_has_nothing_unmerged() {
        let fixture = SubmoduleFixture::with_library();

        assert_eq!(fixture.unmerged_count(), 0);
    }

    #[test]
    fn dirty_state_counts_a_modified_submodule_despite_gitmodules_ignore_all() {
        let fixture = SubmoduleFixture::with_library();
        fixture.git_in_worktree(&[
            "config",
            "-f",
            ".gitmodules",
            "submodule.vendor/library.ignore",
            "all",
        ]);
        fixture.git_in_worktree(&["commit", "-q", "-am", "Ignore the library"]);
        fixture.commit_in_library();

        let dirty = fixture
            .repo
            .workspace()
            .dirty_state(&fixture.worktree)
            .unwrap();

        assert_eq!(dirty.dirty_files(), 1);
    }

    #[test]
    fn dirty_state_counts_a_modified_submodule_despite_diff_ignore_submodules() {
        let fixture = SubmoduleFixture::with_library();
        fixture.repo.git(
            fixture.repo.path(),
            &["config", "diff.ignoreSubmodules", "all"],
        );
        fixture.commit_in_library();

        let dirty = fixture
            .repo
            .workspace()
            .dirty_state(&fixture.worktree)
            .unwrap();

        assert_eq!(dirty.dirty_files(), 1);
    }
}
