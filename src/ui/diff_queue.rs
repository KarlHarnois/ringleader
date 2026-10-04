use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use crate::application::{ServiceError, SessionService, Tool};
use crate::domain::DiffStat;

#[derive(Debug, Default)]
pub struct DiffQueue {
    queued: VecDeque<String>,
    diffs: HashMap<String, DiffStat>,
    timed_out: HashSet<String>,
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
        self.timed_out.clear();
    }

    pub fn keep_only(&mut self, names: &[String]) {
        self.diffs.retain(|name, _| names.contains(name));
        self.timed_out.retain(|name| names.contains(name));
    }

    pub fn tick(
        &mut self,
        tick_start: Instant,
        visible: &[String],
        service: &SessionService,
    ) -> Result<(), ServiceError> {
        if self.next_refresh.is_none_or(|due| tick_start >= due) {
            self.next_refresh = Some(tick_start + service.config().intervals().diff_refresh());
            self.queue_unqueued(visible);
        }
        self.queued.retain(|name| visible.contains(name));
        let mut outcome = Ok(());
        for _ in 0..Self::CALLS_PER_TICK {
            if tick_start.elapsed() >= Self::TICK_BUDGET {
                break;
            }
            let Some(name) = self.queued.pop_front() else {
                break;
            };
            outcome = outcome.and(self.measure(&name, service));
        }
        outcome
    }

    fn queue_unqueued(&mut self, visible: &[String]) {
        for name in visible {
            if !self.queued.contains(name) && !self.timed_out.contains(name) {
                self.queued.push_back(name.clone());
            }
        }
    }

    fn measure(&mut self, name: &str, service: &SessionService) -> Result<(), ServiceError> {
        for (name, result) in service.diff_stats(&[name.to_string()]) {
            match result {
                Ok(diff) => {
                    self.diffs.insert(name, diff);
                }
                Err(error) => {
                    log::warn!("could not measure the diff of {name}: {error}");
                    if error.is_timeout_of(Tool::Git) {
                        self.diffs.remove(&name);
                        self.timed_out.insert(name);
                    }
                    return Err(error);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::WorkspaceError;
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

        queue
            .tick(
                Instant::now(),
                &QueueFixture::visible(&QueueFixture::NAMES),
                &service,
            )
            .unwrap();

        assert_eq!(QueueFixture::diff_calls(&fixture), 2);
        assert_eq!(queue.diffs()["alpha"].untracked(), 4);
    }

    #[test]
    fn the_next_tick_measures_the_rest_of_the_queue() {
        let fixture = QueueFixture::fixture();
        let service = fixture.service();
        let mut queue = DiffQueue::default();
        let visible = QueueFixture::visible(&QueueFixture::NAMES);

        queue.tick(Instant::now(), &visible, &service).unwrap();
        queue.tick(Instant::now(), &visible, &service).unwrap();

        assert_eq!(QueueFixture::diff_calls(&fixture), 3);
        assert_eq!(queue.diffs().len(), 3);
    }

    #[test]
    fn no_call_starts_once_fifty_milliseconds_of_the_tick_passed() {
        let fixture = QueueFixture::fixture();
        let service = fixture.service();
        let mut queue = DiffQueue::default();
        let late_start = Instant::now() - Duration::from_millis(60);

        queue
            .tick(
                late_start,
                &QueueFixture::visible(&QueueFixture::NAMES),
                &service,
            )
            .unwrap();

        assert_eq!(QueueFixture::diff_calls(&fixture), 0);
    }

    #[test]
    fn a_row_that_leaves_the_view_leaves_the_queue() {
        let fixture = QueueFixture::fixture();
        let service = fixture.service();
        let mut queue = DiffQueue::default();
        let late_start = Instant::now() - Duration::from_millis(60);
        queue
            .tick(
                late_start,
                &QueueFixture::visible(&QueueFixture::NAMES),
                &service,
            )
            .unwrap();

        queue
            .tick(Instant::now(), &QueueFixture::visible(&["gamma"]), &service)
            .unwrap();
        queue
            .tick(Instant::now(), &QueueFixture::visible(&["gamma"]), &service)
            .unwrap();

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

        queue.tick(late_start, &visible, &service).unwrap();
        queue.refresh_soon();
        queue.tick(late_start, &visible, &service).unwrap();

        assert_eq!(queue.queued.len(), 3);
    }

    #[test]
    fn rows_are_queued_again_only_after_the_refresh_interval() {
        let fixture = QueueFixture::fixture();
        let service = fixture.service();
        let mut queue = DiffQueue::default();
        let start = Instant::now();
        let visible = QueueFixture::visible(&["alpha"]);

        queue.tick(start, &visible, &service).unwrap();
        queue
            .tick(start + Duration::from_secs(1), &visible, &service)
            .unwrap();
        let before_the_interval = QueueFixture::diff_calls(&fixture);
        queue
            .tick(Instant::now() + Duration::from_secs(4), &visible, &service)
            .unwrap();

        assert_eq!(before_the_interval, 1);
        assert_eq!(QueueFixture::diff_calls(&fixture), 2);
    }

    #[test]
    fn a_timed_out_row_shows_no_diff_and_waits_for_r() {
        let session = ServiceFixture::session("alpha");
        let workspace = ServiceFixture::workspace_for(&[&session]).failing(
            "diff_stat",
            WorkspaceError::TimedOut {
                subcommand: "diff".to_string(),
                seconds: 5,
            },
        );
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_workspace(workspace);
        let service = fixture.service();
        let mut queue = DiffQueue::default();
        let visible = QueueFixture::visible(&["alpha"]);

        let outcome = queue.tick(Instant::now(), &visible, &service);
        queue
            .tick(Instant::now() + Duration::from_secs(4), &visible, &service)
            .ok();
        let before_r = QueueFixture::diff_calls(&fixture);
        queue.refresh_soon();
        queue.tick(Instant::now(), &visible, &service).ok();

        assert!(outcome.unwrap_err().is_timeout_of(Tool::Git));
        assert!(queue.diffs().is_empty());
        assert_eq!(before_r, 1);
        assert_eq!(QueueFixture::diff_calls(&fixture), 2);
    }

    #[test]
    fn diffs_of_sessions_that_are_gone_are_forgotten() {
        let fixture = QueueFixture::fixture();
        let service = fixture.service();
        let mut queue = DiffQueue::default();
        queue
            .tick(Instant::now(), &QueueFixture::visible(&["alpha"]), &service)
            .unwrap();

        queue.keep_only(&QueueFixture::visible(&["beta"]));

        assert!(queue.diffs().is_empty());
    }
}
