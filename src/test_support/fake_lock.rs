use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::application::ports::{Lock, LockError};

pub struct FakeLock {
    held_elsewhere: bool,
    acquisitions: Vec<&'static str>,
    log: Option<Arc<Mutex<Vec<String>>>>,
}

impl FakeLock {
    pub fn free() -> Self {
        Self {
            held_elsewhere: false,
            acquisitions: Vec::new(),
            log: None,
        }
    }

    pub fn held_elsewhere() -> Self {
        Self {
            held_elsewhere: true,
            ..Self::free()
        }
    }

    pub fn recording(self, log: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            log: Some(log),
            ..self
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
        self.record(&format!("acquire {kind}"));
        Ok(())
    }

    fn record(&self, event: &str) {
        if let Some(log) = &self.log {
            log.lock().unwrap().push(event.to_string());
        }
    }
}

impl Lock for FakeLock {
    fn acquire_lifetime(&mut self) -> Result<(), LockError> {
        self.acquire("lifetime")
    }

    fn acquire_for_write(&mut self) -> Result<(), LockError> {
        self.acquire("write")
    }

    fn release(&mut self) {
        self.record("release");
    }
}
