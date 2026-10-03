use std::path::PathBuf;

use crate::application::ports::{Lock, LockError};

pub struct FakeLock {
    held_elsewhere: bool,
    acquisitions: Vec<&'static str>,
}

impl FakeLock {
    pub fn free() -> Self {
        Self {
            held_elsewhere: false,
            acquisitions: Vec::new(),
        }
    }

    pub fn held_elsewhere() -> Self {
        Self {
            held_elsewhere: true,
            acquisitions: Vec::new(),
        }
    }

    pub fn acquisitions(&self) -> &[&'static str] {
        &self.acquisitions
    }

    fn acquire(&mut self, kind: &'static str) -> Result<(), LockError> {
        if self.held_elsewhere {
            return Err(LockError::Held {
                path: PathBuf::from("/state/ringleader/lock"),
            });
        }
        self.acquisitions.push(kind);
        Ok(())
    }
}

impl Lock for FakeLock {
    fn acquire_lifetime(&mut self) -> Result<(), LockError> {
        self.acquire("lifetime")
    }

    fn acquire_for_write(&mut self) -> Result<(), LockError> {
        self.acquire("write")
    }
}
