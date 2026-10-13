use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

pub struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    pub fn open() -> Self {
        Self {
            open: Mutex::new(true),
            changed: Condvar::new(),
        }
    }

    pub fn closed() -> Self {
        Self {
            open: Mutex::new(false),
            changed: Condvar::new(),
        }
    }

    pub fn open_now(&self) {
        *self.lock() = true;
        self.changed.notify_all();
    }

    pub fn wait(&self) {
        drop(self.changed.wait_while(self.lock(), |open| !*open).unwrap());
    }

    pub fn wait_up_to(&self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        let mut open = self.lock();
        while !*open && Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let (guard, _) = self.changed.wait_timeout(open, remaining).unwrap();
            open = guard;
        }
        *open
    }

    fn lock(&self) -> MutexGuard<'_, bool> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
