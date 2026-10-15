use std::sync::Mutex;

use crate::application::ports::Clock;

pub struct FakeClock {
    now: Mutex<u64>,
}

impl FakeClock {
    pub fn at(now: u64) -> Self {
        Self {
            now: Mutex::new(now),
        }
    }
}

impl Clock for FakeClock {
    fn now_epoch_seconds(&self) -> u64 {
        *self.now.lock().unwrap()
    }
}
