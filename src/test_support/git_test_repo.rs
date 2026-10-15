use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tempfile::TempDir;

use crate::adapters::git::git_invocation::GitInvocation;
use crate::adapters::git::git_runner::GitRunner;
use crate::adapters::{BoundedCommand, Deadlines, GitWorkspace};

static CONTENT_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub struct GitTestRepo {
    _tempdir: TempDir,
    canonical_root: PathBuf,
    repo: PathBuf,
}

impl GitTestRepo {
    pub const BRANCH: &'static str = "main";

    pub fn new() -> Self {
        let fixture = Self::unborn();
        fixture.commit(&fixture.repo, "README.md");
        fixture
    }

    pub fn unborn() -> Self {
        let tempdir = tempfile::tempdir().expect("a temporary directory");
        let canonical_root = fs::canonicalize(tempdir.path()).expect("a canonical temporary path");
        fs::create_dir(canonical_root.join("home")).expect("a fixture home");
        let fixture = Self {
            repo: canonical_root.join("example-project"),
            canonical_root,
            _tempdir: tempdir,
        };
        fixture.init(&fixture.repo);
        fixture
    }

    pub fn root(&self) -> &Path {
        &self.canonical_root
    }

    pub fn path(&self) -> &Path {
        &self.repo
    }

    pub fn environment(&self) -> Vec<(OsString, OsString)> {
        let home = self.canonical_root.join("home");
        [
            ("GIT_CONFIG_GLOBAL", OsString::from("/dev/null")),
            ("GIT_CONFIG_NOSYSTEM", OsString::from("1")),
            ("HOME", home.into_os_string()),
            ("GIT_AUTHOR_NAME", OsString::from("Example Author")),
            ("GIT_AUTHOR_EMAIL", OsString::from("author@example.com")),
            ("GIT_COMMITTER_NAME", OsString::from("Example Author")),
            ("GIT_COMMITTER_EMAIL", OsString::from("author@example.com")),
        ]
        .into_iter()
        .map(|(name, value)| (OsString::from(name), value))
        .collect()
    }

    pub fn workspace(&self) -> GitWorkspace {
        self.workspace_with(Deadlines::standard())
    }

    pub fn workspace_with_deadline(&self, deadline: Duration) -> GitWorkspace {
        self.workspace_with(Deadlines::uniform(deadline))
    }

    pub fn runner(&self) -> GitRunner {
        GitRunner::new(
            &OsString::from("git"),
            BoundedCommand::new(Deadlines::standard()),
        )
        .with_environment(self.environment())
    }

    pub fn git(&self, directory: &Path, args: &[&str]) -> String {
        let output = self.try_git(directory, args);
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    pub fn try_git(&self, directory: &Path, args: &[&str]) -> Output {
        self.hermetic("git")
            .arg("-C")
            .arg(directory)
            .args(args)
            .output()
            .expect("git runs")
    }

    pub fn shell(&self, line: &str) {
        let output = self
            .hermetic("sh")
            .args(["-c", line])
            .output()
            .expect("sh runs");
        assert!(
            output.status.success(),
            "sh -c {line:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn hermetic(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command.envs(self.environment());
        for name in GitInvocation::REPOSITORY_LOCAL_VARIABLES {
            command.env_remove(name);
        }
        command
    }

    pub fn commit(&self, directory: &Path, file: &str) -> String {
        let counter = CONTENT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(file);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("a fixture directory");
        }
        fs::write(&path, format!("{file} {counter}\n")).expect("a fixture file");
        self.git(directory, &["add", file]);
        self.git(
            directory,
            &["commit", "-q", "-m", &format!("Change {file}")],
        );
        self.git(directory, &["rev-parse", "HEAD"])
    }

    pub fn init_repo(&self, name: &str) -> PathBuf {
        let path = self.canonical_root.join(name);
        self.init(&path);
        self.commit(&path, "README.md");
        path
    }

    pub fn sibling(&self, name: &str) -> PathBuf {
        self.canonical_root.join(name)
    }

    pub fn add_worktree(&self, name: &str, branch: &str) -> PathBuf {
        let path = self.canonical_root.join("worktrees").join(name);
        let target = path.to_string_lossy().into_owned();
        self.git(
            &self.repo,
            &["worktree", "add", "-q", "-b", branch, &target],
        );
        path
    }

    pub fn add_submodule(&self, directory: &Path, source: &Path, path: &str) {
        let source = source.to_string_lossy().into_owned();
        self.git(
            directory,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                &source,
                path,
            ],
        );
        self.git(directory, &["commit", "-q", "-m", "Add submodule"]);
    }

    fn workspace_with(&self, deadlines: Deadlines) -> GitWorkspace {
        GitWorkspace::new(&OsString::from("git"), BoundedCommand::new(deadlines))
            .with_environment(self.environment())
    }

    fn init(&self, path: &Path) {
        fs::create_dir_all(path).expect("a fixture repository directory");
        self.git(path, &["init", "-q", "-b", Self::BRANCH]);
    }
}
