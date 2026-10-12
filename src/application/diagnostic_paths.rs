use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticPaths {
    tmux_program: String,
    git_program: String,
    state_directory: PathBuf,
    tmux_socket: PathBuf,
    tmux_config: PathBuf,
    user_tmux_config: Option<PathBuf>,
    socket_in_state_directory: bool,
}

impl DiagnosticPaths {
    const SOCKET_PATH_LIMIT: usize = 100;
    pub const TMUX_FIELD_SEPARATOR: char = ',';
    pub const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
    const UNSOURCEABLE_CHARACTERS: [char; 5] = ['\'', '[', '*', '?', '\\'];

    pub fn new(tmux_program: &str, git_program: &str, state_directory: &Path) -> Self {
        Self {
            tmux_program: tmux_program.to_string(),
            git_program: git_program.to_string(),
            state_directory: state_directory.to_path_buf(),
            tmux_socket: PathBuf::new(),
            tmux_config: PathBuf::new(),
            user_tmux_config: None,
            socket_in_state_directory: false,
        }
    }

    pub fn with_tmux_server(self, socket: &Path, config: &Path) -> Self {
        Self {
            tmux_socket: socket.to_path_buf(),
            tmux_config: config.to_path_buf(),
            ..self
        }
    }

    pub fn with_user_tmux_config(self, user_tmux_config: &Path) -> Self {
        Self {
            user_tmux_config: Some(user_tmux_config.to_path_buf()),
            ..self
        }
    }

    pub fn user_tmux_config_problem(&self) -> Option<String> {
        let path = self.user_tmux_config.as_deref()?;
        (!Self::can_source(path)).then(|| {
            format!(
                "Ringleader's tmux server cannot load {}, because tmux reads ', [, *, ?, and \\ in a source-file path as quoting or a glob pattern; move your config home to a path without them",
                path.display()
            )
        })
    }

    pub fn can_source(path: &Path) -> bool {
        !path
            .to_string_lossy()
            .contains(Self::UNSOURCEABLE_CHARACTERS)
    }

    pub fn with_socket_in_state_directory(self) -> Self {
        Self {
            socket_in_state_directory: true,
            ..self
        }
    }

    pub fn socket_in_state_directory(&self) -> bool {
        self.socket_in_state_directory
    }

    pub fn tmux_program(&self) -> &str {
        &self.tmux_program
    }

    pub fn git_program(&self) -> &str {
        &self.git_program
    }

    pub fn state_directory(&self) -> &Path {
        &self.state_directory
    }

    pub fn tmux_socket(&self) -> &Path {
        &self.tmux_socket
    }

    pub fn tmux_config(&self) -> &Path {
        &self.tmux_config
    }

    pub fn socket_problem(&self) -> Option<String> {
        let socket = self.tmux_socket.display();
        let text = self.tmux_socket.to_string_lossy();
        if text.len() >= Self::SOCKET_PATH_LIMIT {
            return Some(format!(
                "the tmux socket path {socket} is {} bytes, and tmux needs it under {}; set XDG_RUNTIME_DIR to a shorter directory",
                text.len(),
                Self::SOCKET_PATH_LIMIT
            ));
        }
        text.contains(Self::TMUX_FIELD_SEPARATOR).then(|| {
            format!(
                "the tmux socket path {socket} contains a comma, which $TMUX uses to separate its fields; set XDG_RUNTIME_DIR to a directory without one"
            )
        })
    }
}

impl Default for DiagnosticPaths {
    fn default() -> Self {
        Self::new("tmux", "git", Path::new("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct PathsFixture;

    impl PathsFixture {
        fn with_socket(socket: &str) -> DiagnosticPaths {
            DiagnosticPaths::default()
                .with_tmux_server(Path::new(socket), Path::new("/state/ringleader/tmux.conf"))
        }
    }

    #[test]
    fn a_short_socket_path_without_a_comma_has_no_problem() {
        let paths = PathsFixture::with_socket("/run/user/1000/ringleader/tmux/tmux.sock");

        assert_eq!(paths.socket_problem(), None);
    }

    #[test]
    fn a_socket_path_of_a_hundred_bytes_is_refused_with_the_path_named() {
        let socket = format!("/{}/tmux.sock", "x".repeat(89));
        assert_eq!(socket.len(), 100);

        let problem = PathsFixture::with_socket(&socket).socket_problem().unwrap();

        assert!(problem.contains(&socket));
    }

    #[test]
    fn a_user_tmux_config_with_glob_or_quote_characters_cannot_be_sourced() {
        for path in [
            "/home/ex'ample/.config/ringleader/tmux.conf",
            "/home/example/[1]/ringleader/tmux.conf",
            "/home/*/ringleader/tmux.conf",
            "/home/ex?mple/ringleader/tmux.conf",
            "/home/ex\\ample/ringleader/tmux.conf",
        ] {
            let paths = DiagnosticPaths::default().with_user_tmux_config(Path::new(path));

            assert!(!DiagnosticPaths::can_source(Path::new(path)), "{path}");
            assert!(paths.user_tmux_config_problem().unwrap().contains(path));
        }
    }

    #[test]
    fn a_plain_user_tmux_config_path_is_sourced_without_a_problem() {
        let path = Path::new("/home/example/.config/ringleader/tmux.conf");
        let paths = DiagnosticPaths::default().with_user_tmux_config(path);

        assert!(DiagnosticPaths::can_source(path));
        assert_eq!(paths.user_tmux_config_problem(), None);
    }

    #[test]
    fn a_socket_path_with_a_comma_is_refused_with_the_path_named() {
        let problem = PathsFixture::with_socket("/run/a,b/ringleader/tmux/tmux.sock")
            .socket_problem()
            .unwrap();

        assert!(problem.contains("/run/a,b/ringleader/tmux/tmux.sock"));
        assert!(problem.contains("comma"));
    }
}
