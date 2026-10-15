use std::collections::VecDeque;
use std::fs::{self, DirEntry};
use std::path::{Path, PathBuf};

use super::mount_table::MountTable;
use crate::adapters::git::git_invocation::GitInvocation;
use crate::adapters::git::git_porcelain::RepoKind;
use crate::adapters::git::git_runner::GitRunner;
use crate::application::ports::{RepoScanner, ScanError, ScanOutcome};
use crate::domain::{Repo, RepoName, RepoScope};

pub struct DirectoryRepoScanner {
    git: GitRunner,
    visit_cap: usize,
    mount_table: PathBuf,
}

impl DirectoryRepoScanner {
    pub const VISIT_CAP: usize = 5000;
    const PRUNED_NAMES: [&'static str; 4] = ["node_modules", "target", "vendor", "dist"];
    const SYSTEM_DIRECTORIES: [&'static str; 4] = ["/proc", "/sys", "/dev", "/nix/store"];
    const GIT_MARKER: &'static str = ".git";

    pub fn new(git: GitRunner, visit_cap: usize, mount_table: &Path) -> Self {
        Self {
            git,
            visit_cap,
            mount_table: mount_table.to_path_buf(),
        }
    }

    fn walk_up(&self, start: &Path) -> WalkUp {
        let mut candidate = Some(start.to_path_buf());
        while let Some(directory) = candidate {
            let marker = directory.join(Self::GIT_MARKER);
            match fs::metadata(&marker) {
                Ok(metadata) if metadata.is_dir() => return WalkUp::Repo(directory),
                Ok(_) => match self.classify(&directory) {
                    Some(RepoKind::Repository(top_level)) => return WalkUp::Repo(top_level),
                    Some(RepoKind::Submodule(superproject)) => candidate = Some(superproject),
                    Some(RepoKind::LinkedWorktree(common_dir)) => {
                        return self
                            .main_repo_of(&common_dir)
                            .map_or(WalkUp::Unresolved(directory), WalkUp::Repo);
                    }
                    None => return WalkUp::Unresolved(directory),
                },
                Err(_) => candidate = directory.parent().map(Path::to_path_buf),
            }
        }
        WalkUp::NoRepo
    }

    fn classify(&self, directory: &Path) -> Option<RepoKind> {
        let check = GitInvocation::in_repo(directory, "rev-parse").args([
            "--path-format=absolute",
            "--show-toplevel",
            "--git-dir",
            "--git-common-dir",
            "--show-superproject-working-tree",
        ]);
        self.git.run_checked(&check).ok()?.repo_kind()
    }

    fn main_repo_of(&self, common_dir: &Path) -> Option<PathBuf> {
        let parent = common_dir.parent()?;
        let check = GitInvocation::in_repo(parent, "rev-parse").arg("--show-toplevel");
        let top_level = PathBuf::from(self.git.run_checked(&check).ok()?.first_line());
        (fs::canonicalize(&top_level).ok()? == parent).then_some(top_level)
    }

    fn sorted_entries(directory: &Path) -> Option<Vec<DirEntry>> {
        let mut entries = fs::read_dir(directory)
            .ok()?
            .collect::<Result<Vec<DirEntry>, _>>()
            .ok()?;
        entries.sort_by_key(DirEntry::file_name);
        Some(entries)
    }

    fn is_descendable(entry: &DirEntry, mounts: &MountTable) -> bool {
        let path = entry.path();
        if mounts.is_remote_mount(&path) {
            return false;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let hidden_or_pruned = name.starts_with('.') || Self::PRUNED_NAMES.contains(&name.as_ref());
        let system = Self::SYSTEM_DIRECTORIES
            .iter()
            .any(|system| path == Path::new(system));
        !hidden_or_pruned && !system && entry.file_type().is_ok_and(|kind| kind.is_dir())
    }

    fn git_marker(entries: &[DirEntry], mounts: &MountTable) -> Option<GitMarker> {
        let marker = entries
            .iter()
            .find(|entry| entry.file_name() == Self::GIT_MARKER)?;
        if mounts.is_remote_mount(&marker.path()) {
            return None;
        }
        let kind = marker.file_type().ok()?;
        Some(if kind.is_dir() {
            GitMarker::Directory
        } else {
            GitMarker::File
        })
    }

    fn found_repo(&self, directory: &Path, marker: GitMarker) -> Option<Repo> {
        let top_level = match marker {
            GitMarker::Directory => directory.to_path_buf(),
            GitMarker::File => match self.classify(directory)? {
                RepoKind::Repository(top_level) => top_level,
                RepoKind::LinkedWorktree(_) | RepoKind::Submodule(_) => return None,
            },
        };
        Some(Repo::discovered(
            RepoName::of_directory(&top_level).ok()?,
            &top_level,
        ))
    }
}

impl RepoScanner for DirectoryRepoScanner {
    fn resolve_scope(&self, launch_directory: &Path) -> Result<RepoScope, ScanError> {
        let root = fs::canonicalize(launch_directory).map_err(|error| ScanError::Unreadable {
            path: launch_directory.to_path_buf(),
            detail: error.to_string(),
        })?;
        let scope = RepoScope::new(&root);
        Ok(match self.walk_up(&root) {
            WalkUp::Repo(top_level) => scope.inside_repo(&top_level),
            WalkUp::Unresolved(directory) => scope.stopped_at_git_file(&directory),
            WalkUp::NoRepo => scope,
        })
    }

    fn scan(&self, scope: &RepoScope, depth: usize) -> Result<ScanOutcome, ScanError> {
        let mounts = MountTable::read_or_empty(&self.mount_table);
        let root = scope.root();
        if Self::sorted_entries(root).is_none() {
            return Err(ScanError::Unreadable {
                path: root.to_path_buf(),
                detail: "the directory cannot be listed".to_string(),
            });
        }
        let mut queue = VecDeque::from([(root.to_path_buf(), 0)]);
        let mut visited = 0;
        let mut repos: Vec<Repo> = Vec::new();
        while let Some((directory, level)) = queue.pop_front() {
            if visited == self.visit_cap {
                return Ok(ScanOutcome::new(repos, true));
            }
            visited += 1;
            let Some(entries) = Self::sorted_entries(&directory) else {
                continue;
            };
            if level > 0
                && let Some(marker) = Self::git_marker(&entries, &mounts)
            {
                let found = self.found_repo(&directory, marker);
                let unseen =
                    found.filter(|repo| repos.iter().all(|seen| seen.path() != repo.path()));
                repos.extend(unseen);
                continue;
            }
            if level < depth {
                let children = entries
                    .iter()
                    .filter(|entry| Self::is_descendable(entry, &mounts));
                queue.extend(children.map(|entry| (entry.path(), level + 1)));
            }
        }
        Ok(ScanOutcome::new(repos, false))
    }
}

enum WalkUp {
    Repo(PathBuf),
    Unresolved(PathBuf),
    NoRepo,
}

#[derive(Clone, Copy)]
enum GitMarker {
    Directory,
    File,
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::adapters::{BoundedCommand, Deadlines};
    use crate::test_support::GitTestRepo;

    struct ScanFixture {
        git: GitTestRepo,
    }

    impl ScanFixture {
        fn new() -> Self {
            let fixture = Self {
                git: GitTestRepo::new(),
            };
            fs::create_dir_all(fixture.scope()).unwrap();
            fixture
        }

        fn scope(&self) -> PathBuf {
            self.git.sibling("scope")
        }

        fn scanner(&self) -> DirectoryRepoScanner {
            self.scanner_with(
                DirectoryRepoScanner::VISIT_CAP,
                Path::new("/nonexistent/mountinfo"),
            )
        }

        fn scanner_with(&self, cap: usize, mount_table: &Path) -> DirectoryRepoScanner {
            let git = GitRunner::new(
                &OsString::from("git"),
                BoundedCommand::new(Deadlines::standard()),
            )
            .with_environment(self.git.environment());
            DirectoryRepoScanner::new(git, cap, mount_table)
        }

        fn repo_at(&self, relative: &str) -> PathBuf {
            let path = self.scope().join(relative);
            fs::create_dir_all(&path).unwrap();
            self.git.git(&path, &["init", "-q", "-b", "main"]);
            self.git.commit(&path, "README.md");
            path
        }

        fn found_in(&self, scope: &Path) -> Vec<PathBuf> {
            self.found_with(&self.scanner(), scope)
        }

        fn found_with(&self, scanner: &DirectoryRepoScanner, scope: &Path) -> Vec<PathBuf> {
            let resolved = scanner.resolve_scope(scope).unwrap();
            let outcome = scanner.scan(&resolved, 3).unwrap();
            outcome
                .repos()
                .iter()
                .map(|repo| repo.path().to_path_buf())
                .collect()
        }

        fn found(&self) -> Vec<PathBuf> {
            self.found_in(&self.scope())
        }

        fn enclosing_from(&self, launch: &Path) -> Option<PathBuf> {
            let scope = self.scanner().resolve_scope(launch).unwrap();
            scope.enclosing_repo().map(Path::to_path_buf)
        }

        fn sealed_directory_is_still_readable(path: &Path) -> bool {
            fs::read_dir(path).is_ok()
        }
    }

    #[test]
    fn nested_repos_below_the_scope_are_found() {
        let fixture = ScanFixture::new();
        let shallow = fixture.repo_at("example-project");
        let deep = fixture.repo_at("work/clients/example-client");

        assert_eq!(fixture.found(), [shallow, deep]);
    }

    #[test]
    fn a_linked_worktree_is_skipped() {
        let fixture = ScanFixture::new();
        let repo = fixture.repo_at("example-project");
        let worktree = fixture.scope().join("example-worktree");
        fixture.git.git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                &worktree.to_string_lossy(),
            ],
        );

        assert_eq!(fixture.found(), [repo]);
    }

    #[test]
    fn a_separate_git_dir_repository_is_listed() {
        let fixture = ScanFixture::new();
        let work_tree = fixture.scope().join("separated");
        let git_dir = fixture.git.sibling("separated.git");
        fs::create_dir_all(&work_tree).unwrap();
        fixture.git.git(
            &work_tree,
            &[
                "init",
                "-q",
                "-b",
                "main",
                "--separate-git-dir",
                &git_dir.to_string_lossy(),
            ],
        );

        assert_eq!(fixture.found(), [work_tree]);
    }

    #[test]
    fn pruned_and_hidden_directories_are_never_entered() {
        let fixture = ScanFixture::new();
        fixture.repo_at("node_modules/example-package");
        fixture.repo_at(".example-tool");
        fixture.repo_at(".config/example-tool/plugin");
        let visible = fixture.repo_at("example-project");

        assert_eq!(fixture.found(), [visible]);
    }

    #[test]
    fn a_hidden_scope_root_is_walked() {
        let fixture = ScanFixture::new();
        let plugin = fixture.repo_at(".config/example-tool/plugin");

        assert_eq!(fixture.found_in(&fixture.scope().join(".config")), [plugin]);
    }

    #[test]
    fn the_depth_limit_finds_depth_three_and_excludes_depth_four() {
        let fixture = ScanFixture::new();
        let depth_three = fixture.repo_at("a/b/example-project");
        fixture.repo_at("a/b/c/example-project");

        assert_eq!(fixture.found(), [depth_three]);
    }

    #[test]
    fn a_plain_directory_is_not_a_repo() {
        let fixture = ScanFixture::new();
        fs::create_dir_all(fixture.scope().join("notes/drafts")).unwrap();

        assert!(fixture.found().is_empty());
    }

    #[test]
    fn two_repos_with_one_basename_both_appear() {
        let fixture = ScanFixture::new();
        let first = fixture.repo_at("one/example-project");
        let second = fixture.repo_at("two/example-project");

        assert_eq!(fixture.found(), [first, second]);
    }

    #[test]
    fn an_unreadable_directory_beside_a_repo_is_skipped() {
        let fixture = ScanFixture::new();
        let repo = fixture.repo_at("example-project");
        let sealed = fixture.scope().join("sealed");
        fs::create_dir(&sealed).unwrap();
        fs::set_permissions(&sealed, fs::Permissions::from_mode(0o000)).unwrap();
        let readable_anyway = ScanFixture::sealed_directory_is_still_readable(&sealed);

        let found = fixture.found();

        fs::set_permissions(&sealed, fs::Permissions::from_mode(0o755)).unwrap();
        if !readable_anyway {
            assert_eq!(found, [repo]);
        }
    }

    #[test]
    fn an_unreadable_scope_root_fails_the_scan() {
        let fixture = ScanFixture::new();
        let scanner = fixture.scanner();
        let scope = scanner.resolve_scope(&fixture.scope()).unwrap();
        fs::set_permissions(fixture.scope(), fs::Permissions::from_mode(0o000)).unwrap();
        let readable_anyway = ScanFixture::sealed_directory_is_still_readable(&fixture.scope());

        let result = scanner.scan(&scope, 3);

        fs::set_permissions(fixture.scope(), fs::Permissions::from_mode(0o755)).unwrap();
        if !readable_anyway {
            assert!(result.is_err());
        }
    }

    #[test]
    fn a_launch_inside_a_linked_worktree_resolves_to_its_main_repo() {
        let fixture = ScanFixture::new();
        let repo = fixture.repo_at("example-project");
        let worktree = fixture.git.sibling("example-worktree");
        fixture.git.git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                &worktree.to_string_lossy(),
            ],
        );
        fs::create_dir(worktree.join("src")).unwrap();

        assert_eq!(fixture.enclosing_from(&worktree.join("src")), Some(repo));
    }

    #[test]
    fn a_launch_inside_a_submodule_resolves_to_its_superproject() {
        let fixture = ScanFixture::new();
        let library = fixture.git.init_repo("library");
        let repo = fixture.repo_at("example-project");
        fixture.git.add_submodule(&repo, &library, "vendor/library");

        assert_eq!(
            fixture.enclosing_from(&repo.join("vendor/library")),
            Some(repo)
        );
    }

    #[test]
    fn a_launch_inside_a_submodule_of_a_linked_worktree_resolves_to_the_main_repo() {
        let fixture = ScanFixture::new();
        let library = fixture.git.init_repo("library");
        let repo = fixture.repo_at("example-project");
        fixture.git.add_submodule(&repo, &library, "vendor/library");
        let worktree = fixture.git.sibling("example-worktree");
        fixture.git.git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                &worktree.to_string_lossy(),
            ],
        );
        fixture.git.git(
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

        assert_eq!(
            fixture.enclosing_from(&worktree.join("vendor/library")),
            Some(repo)
        );
    }

    #[test]
    fn a_launch_inside_a_worktree_of_a_bare_repo_ends_without_a_repo() {
        let fixture = ScanFixture::new();
        let source = fixture.repo_at("example-project");
        let bare = fixture.git.sibling("bare.git");
        fixture.git.git(
            fixture.git.root(),
            &[
                "clone",
                "-q",
                "--bare",
                &source.to_string_lossy(),
                &bare.to_string_lossy(),
            ],
        );
        let worktree = fixture.git.sibling("bare-worktree");
        fixture.git.git(
            &bare,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                &worktree.to_string_lossy(),
            ],
        );

        let scope = fixture.scanner().resolve_scope(&worktree).unwrap();

        assert_eq!(scope.enclosing_repo(), None);
        assert_eq!(scope.unresolved_git_file(), Some(worktree.as_path()));
    }

    #[test]
    fn a_plain_directory_inside_a_repo_finds_the_repo_and_the_repos_below() {
        let fixture = ScanFixture::new();
        let ancestor = fixture.repo_at("ancestor");
        let nested = fixture.repo_at("ancestor/projects/example-project");
        let launch = ancestor.join("projects");

        assert_eq!(fixture.enclosing_from(&launch), Some(ancestor));
        assert_eq!(fixture.found_in(&launch), [nested]);
    }

    #[test]
    fn a_repo_top_level_finds_its_nested_repo_and_never_its_submodule() {
        let fixture = ScanFixture::new();
        let library = fixture.git.init_repo("library");
        let repo = fixture.repo_at("example-project");
        fixture.git.add_submodule(&repo, &library, "vendor/library");
        let nested = fixture.repo_at("example-project/tools/example-tool");

        assert_eq!(fixture.enclosing_from(&repo), Some(repo.clone()));
        assert_eq!(fixture.found_in(&repo), [nested]);
    }

    #[test]
    fn a_separate_git_dir_top_level_finds_its_nested_repo() {
        let fixture = ScanFixture::new();
        let work_tree = fixture.scope().join("separated");
        let git_dir = fixture.git.sibling("separated.git");
        fs::create_dir_all(&work_tree).unwrap();
        fixture.git.git(
            &work_tree,
            &[
                "init",
                "-q",
                "-b",
                "main",
                "--separate-git-dir",
                &git_dir.to_string_lossy(),
            ],
        );
        let nested = fixture.repo_at("separated/tools/example-tool");

        assert_eq!(fixture.enclosing_from(&work_tree), Some(work_tree.clone()));
        assert_eq!(fixture.found_in(&work_tree), [nested]);
    }

    #[test]
    fn a_tree_larger_than_the_cap_is_cut_short() {
        let fixture = ScanFixture::new();
        for index in 0..12 {
            fs::create_dir_all(fixture.scope().join(format!("directory-{index:02}"))).unwrap();
        }
        let scanner = fixture.scanner_with(5, Path::new("/nonexistent/mountinfo"));
        let scope = scanner.resolve_scope(&fixture.scope()).unwrap();

        assert!(scanner.scan(&scope, 3).unwrap().cut_short());
    }

    #[test]
    fn a_tree_exactly_the_size_of_the_cap_is_scanned_whole() {
        let fixture = ScanFixture::new();
        for index in 0..3 {
            fs::create_dir_all(fixture.scope().join(format!("directory-{index:02}"))).unwrap();
        }
        let scope_and_children = 4;
        let scanner = fixture.scanner_with(scope_and_children, Path::new("/nonexistent/mountinfo"));
        let scope = scanner.resolve_scope(&fixture.scope()).unwrap();

        assert!(!scanner.scan(&scope, 3).unwrap().cut_short());
    }

    #[test]
    fn a_shallow_repo_is_found_before_the_cap_stops_a_wide_branch() {
        let fixture = ScanFixture::new();
        for index in 0..20 {
            fs::create_dir_all(fixture.scope().join(format!("aaa/wide/sub-{index:02}"))).unwrap();
        }
        let shallow = fixture.repo_at("zzz/example-project");
        let scanner = fixture.scanner_with(10, Path::new("/nonexistent/mountinfo"));

        assert_eq!(fixture.found_with(&scanner, &fixture.scope()), [shallow]);
    }

    #[test]
    fn a_fuse_mount_inside_the_scope_is_never_entered() {
        let fixture = ScanFixture::new();
        let mounted = fixture.scope().join("remote files");
        let hidden_by_mount = fixture.repo_at("remote files/example-project");
        let table = fixture.git.sibling("mountinfo");
        let escaped = mounted.to_string_lossy().replace(' ', "\\040");
        fs::write(
            &table,
            format!("41 30 0:40 / {escaped} rw - fuse.sshfs host: rw\n"),
        )
        .unwrap();
        let scanner = fixture.scanner_with(DirectoryRepoScanner::VISIT_CAP, &table);

        assert!(fixture.found_with(&scanner, &fixture.scope()).is_empty());
        assert_eq!(fixture.found_with(&scanner, &mounted), [hidden_by_mount]);
    }
}
