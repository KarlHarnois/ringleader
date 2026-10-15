use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use super::InheritedPath;
use crate::adapters::process_environment::ProcessEnvironment;

pub struct DirenvTestHome {
    directory: TempDir,
}

impl DirenvTestHome {
    const PROGRAM: &'static str = "direnv";
    const ENVRC: &'static str = ".envrc";

    pub fn new() -> Self {
        let home = Self {
            directory: tempfile::tempdir().expect("a temporary direnv home"),
        };
        for directory in [home.data(), home.config(), home.projects()] {
            fs::create_dir_all(directory).expect("the direnv home directories are created");
        }
        home
    }

    pub fn program() -> OsString {
        OsString::from(Self::PROGRAM)
    }

    pub fn environment(&self) -> ProcessEnvironment {
        ProcessEnvironment::new(self.variables(), None)
    }

    pub fn variables(&self) -> Vec<(OsString, OsString)> {
        vec![
            (OsString::from("PATH"), InheritedPath::value()),
            (OsString::from("HOME"), self.home().into_os_string()),
            (
                OsString::from("XDG_DATA_HOME"),
                self.data().into_os_string(),
            ),
            (
                OsString::from("XDG_CONFIG_HOME"),
                self.config().into_os_string(),
            ),
        ]
    }

    pub fn project(&self, name: &str) -> PathBuf {
        let project = self.projects().join(name);
        fs::create_dir_all(&project).expect("the project directory is created");
        project
    }

    pub fn project_with_envrc(&self, name: &str, envrc: &str) -> PathBuf {
        let project = self.project(name);
        fs::write(project.join(Self::ENVRC), envrc).expect("the .envrc is written");
        project
    }

    pub fn allow_as_the_user(&self, project: &Path) {
        self.run_as_the_user("allow", project);
    }

    pub fn deny_as_the_user(&self, project: &Path) {
        self.run_as_the_user("deny", project);
    }

    fn run_as_the_user(&self, subcommand: &str, project: &Path) {
        let status = Command::new(Self::PROGRAM)
            .arg(subcommand)
            .arg(project)
            .env_clear()
            .envs(self.variables())
            .status()
            .expect("direnv runs");
        assert!(
            status.success(),
            "direnv {subcommand} succeeds for {}",
            project.display()
        );
    }

    pub fn allow_list(&self) -> PathBuf {
        self.data().join("direnv").join("allow")
    }

    pub fn config_variable_whitelisting(&self, project: &Path) -> (OsString, OsString) {
        let config = self.directory.path().join("direnv-config");
        fs::create_dir_all(&config).expect("the direnv config directory is created");
        let whitelist = format!("[whitelist]\nprefix = [\"{}\"]\n", project.display());
        fs::write(config.join("direnv.toml"), whitelist).expect("the direnv config is written");
        (OsString::from("DIRENV_CONFIG"), config.into_os_string())
    }

    fn home(&self) -> PathBuf {
        self.directory.path().join("home")
    }

    fn data(&self) -> PathBuf {
        self.home().join("data")
    }

    fn config(&self) -> PathBuf {
        self.home().join("config")
    }

    fn projects(&self) -> PathBuf {
        self.directory.path().join("projects")
    }
}
