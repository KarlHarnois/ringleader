use std::fs::{self, DirBuilder, File, TryLockError};
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

use super::durable_file::DurableFile;
use super::host_path::HostPath;
use crate::application::DiagnosticPaths;

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

    pub fn prepare(&self, config_text: &str) -> io::Result<()> {
        self.prepare_socket_directory()?;
        if let Some(config_directory) = self.config.parent() {
            fs::create_dir_all(config_directory)?;
        }
        DurableFile::new(&self.config).write(config_text.as_bytes())
    }

    pub fn prepare_socket_directory(&self) -> io::Result<()> {
        let socket_directory = self.socket.parent().unwrap_or(Path::new("/"));
        DirBuilder::new()
            .recursive(true)
            .mode(DiagnosticPaths::PRIVATE_DIRECTORY_MODE)
            .create(socket_directory)
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
}
