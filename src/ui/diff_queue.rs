use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use crate::application::SessionService;
use crate::domain::DiffStat;

#[derive(Debug, Default)]
pub struct DiffQueue {
    queued: VecDeque<String>,
    diffs: HashMap<String, DiffStat>,
    next_refresh: Option<Instant>,
}

impl DiffQueue {
    pub const CALLS_PER_TICK: usize = 2;
    pub const TICK_BUDGET: Duration = Duration::from_millis(50);

    pub fn diffs(&self) -> &HashMap<String, DiffStat> {
        &self.diffs
    }

    pub fn refresh_soon(&mut self) {
        self.next_refresh = None;
    }

    pub fn keep_only(&mut self, names: &[String]) {
        self.diffs.retain(|name, _| names.contains(name));
    }

    pub fn tick(&mut self, tick_start: Instant, visible: &[String], service: &SessionService) {
        if self.next_refresh.is_none_or(|due| tick_start >= due) {
            self.next_refresh = Some(tick_start + service.config().intervals().diff_refresh());
            self.queue_unqueued(visible);
        }
        self.queued.retain(|name| visible.contains(name));
        for _ in 0..Self::CALLS_PER_TICK {
            if tick_start.elapsed() >= Self::TICK_BUDGET {
                return;
            }
            let Some(name) = self.queued.pop_front() else {
                return;
            };
            self.measure(&name, service);
        }
    }

    fn queue_unqueued(&mut self, visible: &[String]) {
        for name in visible {
            if !self.queued.contains(name) {
                self.queued.push_back(name.clone());
            }
        }
    }

    fn measure(&mut self, name: &str, service: &SessionService) {
        for (name, result) in service.diff_stats(&[name.to_string()]) {
            match result {
                Ok(diff) => {
                    self.diffs.insert(name, diff);
                }
                Err(error) => log::warn!("could not measure the diff of {name}: {error}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ServiceFixture;

    struct QueueFixture;

    impl QueueFixture {
        const NAMES: [&'static str; 3] = ["alpha", "beta", "gamma"];

        fn fixture() -> ServiceFixture {
            let sessions: Vec<_> = Self::NAMES
                .iter()
                .map(|name| ServiceFixture::session(name))
                .collect();
            let workspace = ServiceFixture::workspace_for(&sessions.iter().collect::<Vec<_>>())
                .with_diff(
                    &ServiceFixture::worktree_path("alpha"),
                    DiffStat::from_changes([], 4),
                );
            ServiceFixture::new()
                .with_sessions(&sessions)
                .with_workspace(workspace)
        }

        fn visible(names: &[&str]) -> Vec<String> {
            names.iter().map(|name| name.to_string()).collect()
        }

        fn diff_calls(fixture: &ServiceFixture) -> usize {
            let calls = fixture.workspace().calls();
            calls
                .iter()
                .filter(|call| call.starts_with("diff_stat"))
                .count()
        }
    }

    #[test]
    fn a_tick_measures_at_most_two_visible_rows() {
        let fixture = QueueFixture::fixture();
        let service = fixture.service();
        let mut queue = DiffQueue::default();

        queue.tick(
            Instant::now(),
            &QueueFixture::visible(&QueueFixture::NAMES),
            &service,
        );

        assert_eq!(QueueFixture::diff_calls(&fixture), 2);
        assert_eq!(queue.diffs()["alpha"].untracked(), 4);
    }

    #[test]
    fn the_next_tick_measures_the_rest_of_the_queue() {
        let fixture = QueueFixture::fixture();
        let service = fixture.service();
        let mut queue = DiffQueue::default();
        let visible = QueueFixture::visible(&QueueFixture::NAMES);

        queue.tick(Instant::now(), &visible, &service);
        queue.tick(Instant::now(), &visible, &service);

        assert_eq!(QueueFixture::diff_calls(&fixture), 3);
        assert_eq!(queue.diffs().len(), 3);
    }

    #[test]
    fn no_call_starts_once_fifty_milliseconds_of_the_tick_passed() {
        let fixture = QueueFixture::fixture();
        let service = fixture.service();
        let mut queue = DiffQueue::default();
        let late_start = Instant::now() - Duration::from_millis(60);

        queue.tick(
            late_start,
            &QueueFixture::visible(&QueueFixture::NAMES),
            &service,
        );

        assert_eq!(QueueFixture::diff_calls(&fixture), 0);
    }

    #[test]
    fn a_row_that_leaves_the_view_leaves_the_queue() {
        let fixture = QueueFixture::fixture();
        let service = fixture.service();
        let mut queue = DiffQueue::default();
        let late_start = Instant::now() - Duration::from_millis(60);
        queue.tick(
            late_start,
            &QueueFixture::visible(&QueueFixture::NAMES),
            &service,
        );

        queue.tick(Instant::now(), &QueueFixture::visible(&["gamma"]), &service);
        queue.tick(Instant::now(), &QueueFixture::visible(&["gamma"]), &service);

        assert_eq!(QueueFixture::diff_calls(&fixture), 1);
        assert_eq!(queue.diffs().keys().collect::<Vec<_>>(), ["gamma"]);
    }

    #[test]
    fn a_queued_row_is_not_queued_twice() {
        let fixture = QueueFixture::fixture();
        let service = fixture.service();
        let mut queue = DiffQueue::default();
        let late_start = Instant::now() - Duration::from_millis(60);
        let visible = QueueFixture::visible(&QueueFixture::NAMES);

        queue.tick(late_start, &visible, &service);
        queue.refresh_soon();
        queue.tick(late_start, &visible, &service);

        assert_eq!(queue.queued.len(), 3);
    }

    #[test]
    fn rows_are_queued_again_only_after_the_refresh_interval() {
        let fixture = QueueFixture::fixture();
        let service = fixture.service();
        let mut queue = DiffQueue::default();
        let start = Instant::now();
        let visible = QueueFixture::visible(&["alpha"]);

        queue.tick(start, &visible, &service);
        queue.tick(start + Duration::from_secs(1), &visible, &service);
        let before_the_interval = QueueFixture::diff_calls(&fixture);
        queue.tick(Instant::now() + Duration::from_secs(4), &visible, &service);

        assert_eq!(before_the_interval, 1);
        assert_eq!(QueueFixture::diff_calls(&fixture), 2);
    }

    #[test]
    fn diffs_of_sessions_that_are_gone_are_forgotten() {
        let fixture = QueueFixture::fixture();
        let service = fixture.service();
        let mut queue = DiffQueue::default();
        queue.tick(Instant::now(), &QueueFixture::visible(&["alpha"]), &service);

        queue.keep_only(&QueueFixture::visible(&["beta"]));

        assert!(queue.diffs().is_empty());
    }
}
