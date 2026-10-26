use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::application::ports::{Lock, LockError};

#[derive(Clone)]
pub struct FakeLock {
    state: Arc<Mutex<LockState>>,
    log: Option<Arc<Mutex<Vec<String>>>>,
}

#[derive(Default)]
struct LockState {
    elsewhere: OtherHolder,
    held: bool,
    acquisitions: Vec<&'static str>,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum OtherHolder {
    #[default]
    None,
    Holding,
    ReleasingWithinPatience,
}

impl FakeLock {
    const PATH: &'static str = "/state/ringleader/lock";
    const TRY: &'static str = "try";
    const WAIT: &'static str = "wait";

    pub fn free() -> Self {
        Self::with_other_holder(OtherHolder::None)
    }

    pub fn held_elsewhere() -> Self {
        Self::with_other_holder(OtherHolder::Holding)
    }

    pub fn held_until_waited() -> Self {
        Self::with_other_holder(OtherHolder::ReleasingWithinPatience)
    }

    fn with_other_holder(elsewhere: OtherHolder) -> Self {
        Self {
            state: Arc::new(Mutex::new(LockState {
                elsewhere,
                ..LockState::default()
            })),
            log: None,
        }
    }

    pub fn recording(self, log: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            log: Some(log),
            ..self
        }
    }

    pub fn is_held(&self) -> bool {
        self.state().held
    }

    pub fn acquisitions(&self) -> Vec<&'static str> {
        self.state().acquisitions.clone()
    }

    fn take(&self, kind: &'static str) -> Result<(), LockError> {
        let mut state = self.state();
        state.acquisitions.push(kind);
        if state.elsewhere != OtherHolder::None {
            return Err(LockError::Held {
                path: PathBuf::from(Self::PATH),
            });
        }
        state.held = true;
        drop(state);
        self.record(&format!("acquire {kind}"));
        Ok(())
    }

    fn outwait_a_releasing_holder(&self) {
        let mut state = self.state();
        if state.elsewhere == OtherHolder::ReleasingWithinPatience {
            state.elsewhere = OtherHolder::None;
        }
    }

    fn record(&self, event: &str) {
        if let Some(log) = &self.log {
            log.lock().unwrap().push(event.to_string());
        }
    }

    fn state(&self) -> MutexGuard<'_, LockState> {
        self.state.lock().unwrap()
    }
}

impl Lock for FakeLock {
    fn try_acquire(&mut self) -> Result<(), LockError> {
        self.take(Self::TRY)
    }

    fn acquire(&mut self, stop: &dyn Fn() -> bool) -> Result<(), LockError> {
        if !stop() {
            self.outwait_a_releasing_holder();
        }
        self.take(Self::WAIT)
    }

    fn release(&mut self) {
        self.state().held = false;
        self.record("release");
    }
}
