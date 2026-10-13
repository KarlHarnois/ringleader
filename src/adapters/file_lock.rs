use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use crate::application::ports::{Lock, LockError};

pub struct FileLock {
    path: PathBuf,
    held: Option<File>,
}

impl FileLock {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            held: None,
        }
    }

    fn acquire(&mut self) -> Result<(), LockError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&self.path)
            .map_err(|error| self.io_error(error.to_string()))?;
        match file.try_lock() {
            Ok(()) => {
                self.held = Some(file);
                Ok(())
            }
            Err(TryLockError::WouldBlock) => Err(LockError::Held {
                path: self.path.clone(),
            }),
            Err(TryLockError::Error(error)) => Err(self.io_error(error.to_string())),
        }
    }

    fn io_error(&self, detail: String) -> LockError {
        LockError::Io {
            path: self.path.clone(),
            detail,
        }
    }
}

impl Lock for FileLock {
    fn acquire_lifetime(&mut self) -> Result<(), LockError> {
        self.acquire()
    }

    fn acquire_for_write(&mut self) -> Result<(), LockError> {
        self.acquire()
    }

    fn release(&mut self) {
        self.held = None;
    }
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;

    struct LockFixture;

    impl LockFixture {
        const FORKED_CHILD_EXEC_LIMIT: Duration = Duration::from_secs(1);

        fn acquired_once_forked_children_exec(path: &Path) -> bool {
            let deadline = Instant::now() + Self::FORKED_CHILD_EXEC_LIMIT;
            while Instant::now() < deadline {
                if FileLock::new(path).acquire_for_write().is_ok() {
                    return true;
                }
                thread::sleep(Duration::from_millis(10));
            }
            false
        }
    }

    #[test]
    fn two_locks_on_one_path_in_one_process_conflict() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("lock");
        let mut first = FileLock::new(&path);
        let mut second = FileLock::new(&path);

        first.acquire_lifetime().unwrap();
        let conflict = second.acquire_for_write();

        assert_eq!(conflict, Err(LockError::Held { path }));
    }

    #[test]
    fn a_dropped_lock_frees_the_path() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("lock");
        let mut first = FileLock::new(&path);
        first.acquire_lifetime().unwrap();
        drop(first);

        assert!(LockFixture::acquired_once_forked_children_exec(&path));
    }

    #[test]
    fn an_unopenable_lock_path_is_an_io_error() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("missing/lock");

        let error = FileLock::new(&path).acquire_lifetime().unwrap_err();

        assert!(matches!(error, LockError::Io { .. }));
    }
}
