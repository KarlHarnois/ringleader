use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::application::ports::{Lock, LockError, LockPatience};

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
    patiences: Vec<LockPatience>,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum OtherHolder {
    #[default]
    None,
    Holding,
    ReleasingAfter(Duration),
}

impl FakeLock {
    const PATH: &'static str = "/state/ringleader/lock";
    const TRY: &'static str = "try";
    const WAIT: &'static str = "wait";
    const HELD_UNTIL_WAITED: Duration = Duration::from_secs(1);

    pub fn free() -> Self {
        Self::with_other_holder(OtherHolder::None)
    }

    pub fn held_elsewhere() -> Self {
        Self::with_other_holder(OtherHolder::Holding)
    }

    pub fn held_for(hold: Duration) -> Self {
        Self::with_other_holder(OtherHolder::ReleasingAfter(hold))
    }

    pub fn held_until_waited() -> Self {
        Self::held_for(Self::HELD_UNTIL_WAITED)
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

    pub fn patiences(&self) -> Vec<LockPatience> {
        self.state().patiences.clone()
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

    fn release_holder_within(&self, patience: LockPatience) {
        let mut state = self.state();
        if let OtherHolder::ReleasingAfter(hold) = state.elsewhere
            && hold <= patience.limit()
        {
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

    fn acquire(
        &mut self,
        patience: LockPatience,
        stop: &dyn Fn() -> bool,
    ) -> Result<(), LockError> {
        self.state().patiences.push(patience);
        if !stop() {
            self.release_holder_within(patience);
        }
        self.take(Self::WAIT)
    }

    fn release(&mut self) {
        self.state().held = false;
        self.record("release");
    }
}
