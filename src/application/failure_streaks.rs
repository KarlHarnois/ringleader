use std::collections::HashSet;
use std::sync::{Mutex, MutexGuard, PoisonError};

#[derive(Default)]
pub struct FailureStreaks {
    failing: Mutex<HashSet<String>>,
}

impl FailureStreaks {
    pub fn warn_once(&self, key: &str, warning: &str) {
        if self.failing().insert(key.to_string()) {
            log::warn!("{warning}");
        }
    }

    pub fn end(&self, key: &str) {
        self.failing().remove(key);
    }

    fn failing(&self) -> MutexGuard<'_, HashSet<String>> {
        self.failing.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::LogCapture;

    #[test]
    fn a_streak_warns_once() {
        let log = LogCapture::installed();
        let streaks = FailureStreaks::default();

        for _ in 0..3 {
            streaks.warn_once("streak-example", "streak-example failed");
        }

        assert_eq!(log.count_containing("streak-example failed"), 1);
    }

    #[test]
    fn a_new_streak_after_one_ends_warns_again() {
        let log = LogCapture::installed();
        let streaks = FailureStreaks::default();

        streaks.warn_once("ended-example", "ended-example failed");
        streaks.end("ended-example");
        streaks.warn_once("ended-example", "ended-example failed");

        assert_eq!(log.count_containing("ended-example failed"), 2);
    }

    #[test]
    fn each_key_has_its_own_streak() {
        let log = LogCapture::installed();
        let streaks = FailureStreaks::default();

        streaks.warn_once("first-key-example", "first-key-example failed");
        streaks.warn_once("second-key-example", "second-key-example failed");

        assert_eq!(log.count_containing("second-key-example failed"), 1);
    }
}
