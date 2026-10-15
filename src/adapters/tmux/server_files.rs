use std::fs::{self, DirBuilder, File, TryLockError};
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

use crate::adapters::filesystem::durable_file::DurableFile;
use crate::adapters::filesystem::host_path::HostPath;
use crate::application::DiagnosticPaths;
use crate::application::ports::MultiplexerError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerFiles {
    socket: PathBuf,
    config: PathBuf,
}

impl ServerFiles {
    const START_LOCK_SUFFIX: &'static str = ".lock";

    pub fn new(socket: &Path, config: &Path) -> Self {
        Self {
            socket: socket.to_path_buf(),
            config: config.to_path_buf(),
        }
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub fn config(&self) -> &Path {
        &self.config
    }

    pub fn prepare(&self, config_text: &str) -> Result<(), MultiplexerError> {
        self.prepare_socket_directory()?;
        if let Some(config_directory) = self.config.parent() {
            fs::create_dir_all(config_directory).map_err(Self::failed_at(config_directory))?;
        }
        DurableFile::new(&self.config)
            .write(config_text.as_bytes())
            .map_err(Self::failed_at(&self.config))
    }

    pub fn prepare_socket_directory(&self) -> Result<(), MultiplexerError> {
        let socket_directory = self.socket.parent().unwrap_or(Path::new("/"));
        DirBuilder::new()
            .recursive(true)
            .mode(DiagnosticPaths::PRIVATE_DIRECTORY_MODE)
            .create(socket_directory)
            .map_err(Self::failed_at(socket_directory))
    }

    fn failed_at(path: &Path) -> impl FnOnce(io::Error) -> MultiplexerError + '_ {
        move |error| MultiplexerError::ServerFiles {
            path: path.to_path_buf(),
            detail: error.to_string(),
        }
    }

    pub fn server_is_starting(&self) -> bool {
        File::open(self.start_lock())
            .is_ok_and(|lock| matches!(lock.try_lock_shared(), Err(TryLockError::WouldBlock)))
    }

    fn start_lock(&self) -> PathBuf {
        let mut lock = self.socket.clone().into_os_string();
        lock.push(Self::START_LOCK_SUFFIX);
        PathBuf::from(lock)
    }

    pub fn is_reached_through(&self, socket: &Path) -> bool {
        HostPath::new(socket).canonical_lenient() == HostPath::new(&self.socket).canonical_lenient()
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn preparing_the_server_files_makes_a_private_socket_directory_and_the_config() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("run/ringleader/tmux/tmux.sock");
        let config = directory.path().join("state/ringleader/tmux.conf");

        ServerFiles::new(&socket, &config)
            .prepare("example\n")
            .unwrap();

        let mode = fs::metadata(socket.parent().unwrap())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
        assert_eq!(fs::read_to_string(&config).unwrap(), "example\n");
    }

    #[test]
    fn a_socket_directory_that_cannot_be_made_is_named_in_the_error() {
        let directory = tempfile::tempdir().unwrap();
        let blocker = directory.path().join("run");
        fs::write(&blocker, "").unwrap();
        let socket = blocker.join("ringleader/tmux/tmux.sock");
        let config = directory.path().join("state/ringleader/tmux.conf");

        let error = ServerFiles::new(&socket, &config)
            .prepare("example\n")
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains(&socket.parent().unwrap().display().to_string())
        );
    }

    #[test]
    fn a_config_directory_that_cannot_be_made_is_named_in_the_error() {
        let directory = tempfile::tempdir().unwrap();
        let blocker = directory.path().join("state");
        fs::write(&blocker, "").unwrap();
        let socket = directory.path().join("run/ringleader/tmux/tmux.sock");
        let config = blocker.join("ringleader/tmux.conf");

        let error = ServerFiles::new(&socket, &config)
            .prepare("example\n")
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains(&config.parent().unwrap().display().to_string())
        );
    }
}
