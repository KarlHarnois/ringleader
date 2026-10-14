use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;

use crate::application::{Pending, ServiceError, SessionService, Tool};
use crate::domain::DiffStat;

#[derive(Default)]
pub struct DiffQueue {
    queued: VecDeque<String>,
    measuring: Vec<Measurement>,
    diffs: HashMap<String, DiffStat>,
    timed_out: HashSet<String>,
    next_refresh: Option<Instant>,
}

impl DiffQueue {
    pub const IN_FLIGHT: usize = 2;

    pub fn diffs(&self) -> &HashMap<String, DiffStat> {
        &self.diffs
    }

    pub fn awaits_results(&self) -> bool {
        !self.measuring.is_empty()
    }

    pub fn refresh_soon(&mut self) {
        self.next_refresh = None;
        self.timed_out.clear();
        self.measuring.iter_mut().for_each(Measurement::supersede);
    }

    pub fn keep_only(&mut self, names: &[String]) {
        self.diffs.retain(|name, _| names.contains(name));
        self.timed_out.retain(|name| names.contains(name));
        self.measuring
            .retain(|measurement| names.contains(&measurement.name));
    }

    pub fn tick(
        &mut self,
        now: Instant,
        visible: &[String],
        service: &SessionService,
    ) -> Result<(), ServiceError> {
        let collected = self.collect();
        if self.is_due(now, visible) {
            self.next_refresh = Some(now + service.config().intervals().diff_refresh());
            self.queue_unqueued(visible);
        }
        self.queued.retain(|name| visible.contains(name));
        self.start_measurements(service);
        collected
    }

    fn collect(&mut self) -> Result<(), ServiceError> {
        let mut outcome = Ok(());
        for measurement in std::mem::take(&mut self.measuring) {
            match measurement.pending.poll() {
                Some(_) if measurement.superseded => {}
                Some(measured) => outcome = outcome.and(self.record(measurement.name, measured)),
                None => self.measuring.push(measurement),
            }
        }
        outcome
    }

    fn record(
        &mut self,
        name: String,
        measured: Result<DiffStat, ServiceError>,
    ) -> Result<(), ServiceError> {
        match measured {
            Ok(diff) => {
                self.diffs.insert(name, diff);
                Ok(())
            }
            Err(error) => {
                self.fail(name, &error);
                Err(error)
            }
        }
    }

    fn fail(&mut self, name: String, error: &ServiceError) {
        log::warn!("could not measure the diff of {name}: {error}");
        if error.is_timeout_of(Tool::Git) {
            self.diffs.remove(&name);
            self.timed_out.insert(name);
        }
    }

    fn is_due(&self, now: Instant, visible: &[String]) -> bool {
        !visible.is_empty() && self.next_refresh.is_none_or(|due| now >= due)
    }

    fn queue_unqueued(&mut self, visible: &[String]) {
        for name in visible {
            if self.awaits_queueing(name) {
                self.queued.push_back(name.clone());
            }
        }
    }

    fn awaits_queueing(&self, name: &String) -> bool {
        let measuring = self
            .measuring
            .iter()
            .any(|measurement| measurement.is_current_for(name));
        !measuring && !self.queued.contains(name) && !self.timed_out.contains(name)
    }

    fn start_measurements(&mut self, service: &SessionService) {
        while self.measuring.len() < Self::IN_FLIGHT {
            let Some(name) = self.queued.pop_front() else {
                return;
            };
            if let Some(pending) = service.start_diff(&name) {
                self.measuring.push(Measurement::new(name, pending));
            }
        }
    }
}

struct Measurement {
    name: String,
    pending: Pending<DiffStat>,
    superseded: bool,
}

impl Measurement {
    fn new(name: String, pending: Pending<DiffStat>) -> Self {
        Self {
            name,
            pending,
            superseded: false,
        }
    }

    fn supersede(&mut self) {
        self.superseded = true;
    }

    fn is_current_for(&self, name: &str) -> bool {
        !self.superseded && self.name == name
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use super::*;
    use crate::application::ports::WorkspaceError;
    use crate::domain::Session;
    use crate::test_support::{Deadline, FakeWorkspace, HeldCalls, ServiceFixture};

    struct QueueFixture;

    impl QueueFixture {
        const NAMES: [&'static str; 3] = ["alpha", "beta", "gamma"];
        const LIMIT: Duration = Duration::from_secs(2);
        const POLL_INTERVAL: Duration = Duration::from_millis(5);

        fn fixture() -> ServiceFixture {
            Self::with_workspace(Self::workspace())
        }

        fn holding(held: &Arc<HeldCalls>) -> ServiceFixture {
            Self::with_workspace(Self::workspace().on_call("diff_stat", held.hook()))
        }

        fn workspace() -> FakeWorkspace {
            ServiceFixture::workspace_for(&Self::sessions().iter().collect::<Vec<_>>()).with_diff(
                &ServiceFixture::worktree_path("alpha"),
                DiffStat::from_changes([], 4),
            )
        }

        fn with_workspace(workspace: FakeWorkspace) -> ServiceFixture {
            ServiceFixture::new()
                .with_sessions(&Self::sessions())
                .with_workspace(workspace)
        }

        fn sessions() -> Vec<Session> {
            Self::NAMES
                .iter()
                .map(|name| ServiceFixture::session(name))
                .collect()
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

        fn wait_for_diff_calls(fixture: &ServiceFixture, count: usize) -> bool {
            Deadline::after(Self::LIMIT).wait_until(|| Self::diff_calls(fixture) == count)
        }

        fn settle_at(
            queue: &mut DiffQueue,
            now: Instant,
            visible: &[String],
            service: &SessionService,
        ) -> Result<(), ServiceError> {
            let deadline = Instant::now() + Self::LIMIT;
            let mut outcome = queue.tick(now, visible, service);
            while queue.awaits_results() && Instant::now() < deadline {
                thread::sleep(Self::POLL_INTERVAL);
                outcome = outcome.and(queue.tick(now, visible, service));
            }
            outcome
        }
    }

    #[test]
    fn finished_measurements_make_room_for_the_rest_of_the_queue() {
        let fixture = QueueFixture::fixture();
        let service = fixture.service();
        let mut queue = DiffQueue::default();
        let visible = QueueFixture::visible(&QueueFixture::NAMES);

        QueueFixture::settle_at(&mut queue, Instant::now(), &visible, &service).unwrap();

        assert_eq!(QueueFixture::diff_calls(&fixture), 3);
        assert_eq!(queue.diffs().len(), 3);
        assert_eq!(queue.diffs()["alpha"].untracked(), 4);
    }

    #[test]
    fn no_more_than_two_measurements_run_at_once() {
        let held = HeldCalls::all();
        let fixture = QueueFixture::holding(&held);
        let service = fixture.service();
        let mut queue = DiffQueue::default();
        let visible = QueueFixture::visible(&QueueFixture::NAMES);

        for _ in 0..3 {
            queue.tick(Instant::now(), &visible, &service).unwrap();
            queue.refresh_soon();
        }
        let two_started = held.wait_for(2);
        let only_two = held.stays_at(2);
        held.release();

        assert!(two_started);
        assert!(only_two);
    }

    #[test]
    fn a_row_that_leaves_the_view_before_its_turn_is_not_measured() {
        let fixture = QueueFixture::fixture();
        let service = fixture.service();
        let mut queue = DiffQueue::default();
        let start = Instant::now();
        queue
            .tick(
                start,
                &QueueFixture::visible(&QueueFixture::NAMES),
                &service,
            )
            .unwrap();

        QueueFixture::settle_at(
            &mut queue,
            start,
            &QueueFixture::visible(&["alpha"]),
            &service,
        )
        .unwrap();

        assert_eq!(QueueFixture::diff_calls(&fixture), 2);
        assert!(!queue.diffs().contains_key("gamma"));
    }

    #[test]
    fn a_queued_or_measuring_row_is_not_queued_twice() {
        let held = HeldCalls::all();
        let fixture = QueueFixture::holding(&held);
        let service = fixture.service();
        let mut queue = DiffQueue::default();
        let visible = QueueFixture::visible(&QueueFixture::NAMES);
        let start = Instant::now();

        queue.tick(start, &visible, &service).unwrap();
        queue
            .tick(start + Duration::from_secs(4), &visible, &service)
            .unwrap();
        held.release();

        assert_eq!(queue.queued, ["gamma"]);
    }

    #[test]
    fn a_refresh_replaces_a_measurement_that_was_in_flight() {
        let held = HeldCalls::first(1);
        let fixture = QueueFixture::holding(&held);
        let service = fixture.service();
        let mut queue = DiffQueue::default();
        let visible = QueueFixture::visible(&["alpha"]);
        let start = Instant::now();
        queue.tick(start, &visible, &service).unwrap();
        held.wait_for(1);

        fixture.workspace().change_diff(
            &ServiceFixture::worktree_path("alpha"),
            DiffStat::from_changes([], 9),
        );
        queue.refresh_soon();
        queue.tick(start, &visible, &service).unwrap();
        QueueFixture::wait_for_diff_calls(&fixture, 1);
        queue.tick(start, &visible, &service).unwrap();
        held.release();
        QueueFixture::settle_at(&mut queue, start, &visible, &service).unwrap();

        assert_eq!(queue.diffs()["alpha"].untracked(), 9);
    }

    #[test]
    fn rows_are_queued_again_only_after_the_refresh_interval() {
        let fixture = QueueFixture::fixture();
        let service = fixture.service();
        let mut queue = DiffQueue::default();
        let start = Instant::now();
        let visible = QueueFixture::visible(&["alpha"]);

        QueueFixture::settle_at(&mut queue, start, &visible, &service).unwrap();
        QueueFixture::settle_at(
            &mut queue,
            start + Duration::from_secs(1),
            &visible,
            &service,
        )
        .unwrap();
        let before_the_interval = QueueFixture::diff_calls(&fixture);
        QueueFixture::settle_at(
            &mut queue,
            Instant::now() + Duration::from_secs(4),
            &visible,
            &service,
        )
        .unwrap();

        assert_eq!(before_the_interval, 1);
        assert_eq!(QueueFixture::diff_calls(&fixture), 2);
    }

    #[test]
    fn an_empty_view_leaves_the_refresh_due() {
        let fixture = QueueFixture::fixture();
        let service = fixture.service();
        let mut queue = DiffQueue::default();
        let start = Instant::now();

        queue.tick(start, &[], &service).unwrap();
        queue
            .tick(start, &QueueFixture::visible(&["alpha"]), &service)
            .unwrap();

        assert!(queue.awaits_results());
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

        let outcome = QueueFixture::settle_at(&mut queue, Instant::now(), &visible, &service);
        let later = Instant::now() + Duration::from_secs(4);
        QueueFixture::settle_at(&mut queue, later, &visible, &service).ok();
        let before_r = QueueFixture::diff_calls(&fixture);
        queue.refresh_soon();
        QueueFixture::settle_at(&mut queue, Instant::now(), &visible, &service).ok();

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
        let visible = QueueFixture::visible(&["alpha"]);
        QueueFixture::settle_at(&mut queue, Instant::now(), &visible, &service).unwrap();

        queue.keep_only(&QueueFixture::visible(&["beta"]));

        assert!(queue.diffs().is_empty());
    }

    #[test]
    fn a_measurement_of_a_session_that_is_gone_is_dropped() {
        let held = HeldCalls::all();
        let fixture = QueueFixture::holding(&held);
        let service = fixture.service();
        let mut queue = DiffQueue::default();
        let visible = QueueFixture::visible(&["alpha"]);
        queue.tick(Instant::now(), &visible, &service).unwrap();

        queue.keep_only(&QueueFixture::visible(&["beta"]));
        held.release();
        QueueFixture::settle_at(&mut queue, Instant::now(), &[], &service).unwrap();

        assert!(queue.diffs().is_empty());
    }
}
