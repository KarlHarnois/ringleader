use std::thread;
use std::time::{Duration, Instant};

pub struct Deadline {
    end: Instant,
}

impl Deadline {
    const POLL_INTERVAL: Duration = Duration::from_millis(5);
    const BACKGROUND_WORK_LIMIT: Duration = Duration::from_secs(2);

    pub fn after(limit: Duration) -> Self {
        Self {
            end: Instant::now() + limit,
        }
    }

    pub fn for_background_work() -> Self {
        Self::after(Self::BACKGROUND_WORK_LIMIT)
    }

    pub fn wait_until(&self, mut condition: impl FnMut() -> bool) -> bool {
        while Instant::now() < self.end {
            if condition() {
                return true;
            }
            thread::sleep(Self::POLL_INTERVAL);
        }
        condition()
    }
}
