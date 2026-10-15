use std::time::{SystemTime, UNIX_EPOCH};

use crate::application::ports::Clock;

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_epoch_seconds(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_reads_seconds_after_the_epoch() {
        assert!(SystemClock.now_epoch_seconds() > 1_700_000_000);
    }
}
