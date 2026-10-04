use std::path::{Path, PathBuf};

use super::process_environment::ProcessEnvironment;

pub struct XdgPaths {
    home: Option<PathBuf>,
    config_home: PathBuf,
    state_home: PathBuf,
    data_home: PathBuf,
    runtime_directory: Option<PathBuf>,
}

impl XdgPaths {
    const APPLICATION: &'static str = "ringleader";

    pub fn from_environment(environment: &ProcessEnvironment) -> Self {
        let home = environment
            .get("HOME")
            .map(PathBuf::from)
            .or_else(dirs::home_dir);
        let base = |variable: &str, fallback: &str| {
            environment
                .get(variable)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .unwrap_or_else(|| home.clone().unwrap_or_default().join(fallback))
        };
        Self {
            config_home: base("XDG_CONFIG_HOME", ".config"),
            state_home: base("XDG_STATE_HOME", ".local/state"),
            data_home: base("XDG_DATA_HOME", ".local/share"),
            runtime_directory: environment
                .get("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute()),
            home,
        }
    }

    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_home.join(Self::APPLICATION).join("config.toml")
    }

    pub fn state_directory(&self) -> PathBuf {
        self.state_home.join(Self::APPLICATION)
    }

    pub fn worktree_root(&self) -> PathBuf {
        self.data_home.join(Self::APPLICATION).join("worktrees")
    }

    pub fn handoff_directory(&self) -> PathBuf {
        let base = self.runtime_directory.as_ref().unwrap_or(&self.state_home);
        base.join(Self::APPLICATION).join("handoff")
    }
}
