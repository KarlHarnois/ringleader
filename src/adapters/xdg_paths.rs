use std::path::{Path, PathBuf};

use super::host_path::HostPath;
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

    pub fn user_tmux_config(&self) -> PathBuf {
        self.config_home.join(Self::APPLICATION).join("tmux.conf")
    }

    pub fn state_directory(&self) -> PathBuf {
        self.state_home.join(Self::APPLICATION)
    }

    pub fn worktree_root(&self) -> PathBuf {
        self.data_home.join(Self::APPLICATION).join("worktrees")
    }

    pub fn handoff_directory(&self) -> PathBuf {
        self.runtime_application_directory().join("handoff")
    }

    pub fn tmux_socket(&self) -> PathBuf {
        let directory = self.runtime_application_directory().join("tmux");
        HostPath::new(&directory)
            .canonical_lenient()
            .join("tmux.sock")
    }

    pub fn tmux_config(&self) -> PathBuf {
        self.state_directory().join("tmux.conf")
    }

    pub fn has_runtime_directory(&self) -> bool {
        self.runtime_directory.is_some()
    }

    fn runtime_application_directory(&self) -> PathBuf {
        let base = self.runtime_directory.as_ref().unwrap_or(&self.state_home);
        base.join(Self::APPLICATION)
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    struct PathsFixture;

    impl PathsFixture {
        fn with(pairs: &[(&str, &str)]) -> XdgPaths {
            let variables = pairs
                .iter()
                .map(|(name, value)| (OsString::from(name), OsString::from(value)))
                .collect();
            XdgPaths::from_environment(&ProcessEnvironment::new(variables, None))
        }
    }

    #[test]
    fn the_tmux_socket_lives_in_the_runtime_directory() {
        let paths = PathsFixture::with(&[
            ("XDG_RUNTIME_DIR", "/example-runtime"),
            ("XDG_STATE_HOME", "/example-state"),
        ]);

        assert_eq!(
            paths.tmux_socket(),
            Path::new("/example-runtime/ringleader/tmux/tmux.sock")
        );
    }

    #[test]
    fn an_unset_or_relative_runtime_directory_falls_back_to_the_state_directory() {
        for runtime in [None, Some("relative-runtime")] {
            let mut pairs = vec![("XDG_STATE_HOME", "/example-state")];
            pairs.extend(runtime.map(|value| ("XDG_RUNTIME_DIR", value)));

            let socket = PathsFixture::with(&pairs).tmux_socket();

            assert_eq!(
                socket,
                Path::new("/example-state/ringleader/tmux/tmux.sock")
            );
        }
    }

    #[test]
    fn the_tmux_config_sits_beside_the_other_state_files() {
        let paths = PathsFixture::with(&[("XDG_STATE_HOME", "/example-state")]);

        assert_eq!(
            paths.tmux_config(),
            Path::new("/example-state/ringleader/tmux.conf")
        );
    }

    #[test]
    fn the_user_tmux_config_lives_in_the_config_home() {
        let paths = PathsFixture::with(&[("XDG_CONFIG_HOME", "/example-config")]);

        assert_eq!(
            paths.user_tmux_config(),
            Path::new("/example-config/ringleader/tmux.conf")
        );
    }

    #[test]
    fn a_symlinked_runtime_directory_is_resolved() {
        let directory = tempfile::tempdir().unwrap();
        let real = std::fs::canonicalize(directory.path())
            .unwrap()
            .join("real");
        std::fs::create_dir(&real).unwrap();
        let link = directory.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let socket =
            PathsFixture::with(&[("XDG_RUNTIME_DIR", &link.to_string_lossy())]).tmux_socket();

        assert_eq!(socket, real.join("ringleader/tmux/tmux.sock"));
    }
}
