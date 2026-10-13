use std::thread;
use std::time::{Duration, Instant};

pub struct Deadline {
    end: Instant,
}

impl Deadline {
    const POLL_INTERVAL: Duration = Duration::from_millis(5);

    pub fn after(limit: Duration) -> Self {
        Self {
            end: Instant::now() + limit,
        }
    }

    pub fn wait_until(&self, condition: impl Fn() -> bool) -> bool {
        while Instant::now() < self.end {
            if condition() {
                return true;
            }
            thread::sleep(Self::POLL_INTERVAL);
        }
        condition()
    }
}
