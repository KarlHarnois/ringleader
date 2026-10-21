use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use crate::application::ports::{Lock, LockError};

pub struct FileLock {
    path: PathBuf,
    retries: u32,
    held: Option<File>,
}

impl FileLock {
    const RETRIES: u32 = 600;
    const RETRY_INTERVAL: Duration = Duration::from_millis(100);

    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            retries: Self::RETRIES,
            held: None,
        }
    }

    #[cfg(test)]
    pub fn with_retries(self, retries: u32) -> Self {
        Self { retries, ..self }
    }

    fn open(&self) -> Result<File, LockError> {
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&self.path)
            .map_err(|error| self.io_error(error.to_string()))
    }

    fn io_error(&self, detail: String) -> LockError {
        LockError::Io {
            path: self.path.clone(),
            detail,
        }
    }
}

impl Lock for FileLock {
    fn try_acquire(&mut self) -> Result<(), LockError> {
        let file = self.open()?;
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

    fn acquire(&mut self, stop: &dyn Fn() -> bool) -> Result<(), LockError> {
        for _ in 0..self.retries {
            match self.try_acquire() {
                Err(LockError::Held { .. }) if !stop() => thread::sleep(Self::RETRY_INTERVAL),
                attempt => return attempt,
            }
        }
        self.try_acquire()
    }

    fn release(&mut self) {
        self.held = None;
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;

    struct LockFixture {
        directory: tempfile::TempDir,
    }

    impl LockFixture {
        const FORKED_CHILD_EXEC_LIMIT: Duration = Duration::from_secs(1);
        const FEW_RETRIES: u32 = 3;
        const ENOUGH_RETRIES_TO_OUTLAST_THE_HOLD: u32 = 20;
        const HOLD: Duration = Duration::from_millis(150);

        fn new() -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
            }
        }

        fn path(&self) -> PathBuf {
            self.directory.path().join("lock")
        }

        fn lock(&self) -> FileLock {
            FileLock::new(&self.path()).with_retries(Self::FEW_RETRIES)
        }

        fn patient_lock(&self) -> FileLock {
            FileLock::new(&self.path()).with_retries(Self::ENOUGH_RETRIES_TO_OUTLAST_THE_HOLD)
        }

        fn held(&self) -> FileLock {
            let mut holder = self.lock();
            holder.try_acquire().unwrap();
            holder
        }

        fn release_later(mut holder: FileLock) -> thread::JoinHandle<()> {
            thread::spawn(move || {
                thread::sleep(Self::HOLD);
                holder.release();
            })
        }

        fn never() -> bool {
            false
        }

        fn acquired_once_forked_children_exec(&self) -> bool {
            let deadline = Instant::now() + Self::FORKED_CHILD_EXEC_LIMIT;
            while Instant::now() < deadline {
                if self.lock().try_acquire().is_ok() {
                    return true;
                }
                thread::sleep(Duration::from_millis(10));
            }
            false
        }
    }

    #[test]
    fn two_locks_on_one_path_in_one_process_conflict() {
        let fixture = LockFixture::new();
        let _holder = fixture.held();

        let conflict = fixture.lock().try_acquire();

        assert_eq!(
            conflict,
            Err(LockError::Held {
                path: fixture.path()
            })
        );
    }

    #[test]
    fn a_dropped_lock_frees_the_path() {
        let fixture = LockFixture::new();
        drop(fixture.held());

        assert!(fixture.acquired_once_forked_children_exec());
    }

    #[test]
    fn a_released_lock_frees_the_path() {
        let fixture = LockFixture::new();
        let mut holder = fixture.held();

        holder.release();

        assert!(fixture.acquired_once_forked_children_exec());
    }

    #[test]
    fn acquire_waits_for_the_holder_to_release() {
        let fixture = LockFixture::new();
        let release = LockFixture::release_later(fixture.held());

        let acquired = fixture.patient_lock().acquire(&LockFixture::never);

        release.join().unwrap();
        assert_eq!(acquired, Ok(()));
    }

    #[test]
    fn acquire_gives_up_once_its_retries_run_out() {
        let fixture = LockFixture::new();
        let _holder = fixture.held();

        let acquired = fixture.lock().acquire(&LockFixture::never);

        assert_eq!(
            acquired,
            Err(LockError::Held {
                path: fixture.path()
            })
        );
    }

    #[test]
    fn acquire_stops_waiting_when_asked() {
        let fixture = LockFixture::new();
        let _holder = fixture.held();
        let asked = Cell::new(0);

        let acquired = fixture.lock().acquire(&|| {
            asked.set(asked.get() + 1);
            true
        });

        assert!(matches!(acquired, Err(LockError::Held { .. })));
        assert_eq!(asked.get(), 1);
    }

    #[test]
    fn acquire_asks_whether_to_stop_before_each_retry() {
        let fixture = LockFixture::new();
        let _holder = fixture.held();
        let asked = Cell::new(0);

        let _ = fixture.lock().acquire(&|| {
            asked.set(asked.get() + 1);
            false
        });

        assert_eq!(asked.get(), LockFixture::FEW_RETRIES);
    }

    #[test]
    fn an_unopenable_lock_path_is_an_io_error() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("missing/lock");

        let error = FileLock::new(&path).try_acquire().unwrap_err();

        assert!(matches!(error, LockError::Io { .. }));
    }
}
