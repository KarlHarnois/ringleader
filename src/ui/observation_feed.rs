use std::time::Instant;

use crate::application::{Pending, ServiceError, SessionService};
use crate::domain::SessionObservation;

pub struct ObservationFeed {
    pending: Option<Pending<Vec<SessionObservation>>>,
    next_observe: Instant,
}

impl ObservationFeed {
    pub fn new(now: Instant) -> Self {
        Self {
            pending: None,
            next_observe: now,
        }
    }

    pub fn awaits_result(&self) -> bool {
        self.pending.is_some()
    }

    pub fn start_if_due(&mut self, now: Instant, service: &SessionService) {
        if self.pending.is_some() || now < self.next_observe {
            return;
        }
        self.next_observe = now + service.config().intervals().state_poll();
        self.pending = Some(service.start_observing());
    }

    pub fn collect(&mut self) -> Option<Result<Vec<SessionObservation>, ServiceError>> {
        let finished = self.pending.as_ref()?.poll()?;
        self.pending = None;
        Some(finished)
    }

    pub fn supersede(&mut self, now: Instant, service: &SessionService) {
        self.pending = None;
        self.next_observe = now + service.config().intervals().state_poll();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;
    use crate::test_support::{Deadline, FakeMultiplexer, HeldCalls, ServiceFixture};

    struct FeedFixture;

    impl FeedFixture {
        fn holding(held: &Arc<HeldCalls>) -> ServiceFixture {
            ServiceFixture::new()
                .with_multiplexer(FakeMultiplexer::new().on_call("observe_all", held.hook()))
        }

        fn wait_for_the_listing(fixture: &ServiceFixture) {
            Deadline::for_background_work()
                .wait_until(|| !fixture.multiplexer().calls().is_empty());
        }
    }

    #[test]
    fn a_due_feed_starts_no_second_observation_while_one_runs() {
        let held = HeldCalls::all();
        let service = FeedFixture::holding(&held).service();
        let start = Instant::now();
        let mut feed = ObservationFeed::new(start);

        feed.start_if_due(start, &service);
        feed.start_if_due(start + Duration::from_secs(5), &service);
        let one_started = held.wait_for(1);
        let only_one = held.stays_at(1);
        held.release();

        assert!(one_started);
        assert!(only_one);
    }

    #[test]
    fn a_superseded_feed_waits_a_full_interval_before_observing_again() {
        let service = ServiceFixture::new().service();
        let start = Instant::now();
        let mut feed = ObservationFeed::new(start);

        feed.supersede(start, &service);
        feed.start_if_due(start, &service);
        let started_at_once = feed.awaits_result();
        feed.start_if_due(start + Duration::from_secs(1), &service);

        assert!(!started_at_once);
        assert!(feed.awaits_result());
    }

    #[test]
    fn a_superseded_observation_is_never_collected() {
        let held = HeldCalls::all();
        let fixture = FeedFixture::holding(&held);
        let service = fixture.service();
        let start = Instant::now();
        let mut feed = ObservationFeed::new(start);
        feed.start_if_due(start, &service);
        held.wait_for(1);

        feed.supersede(start, &service);
        held.release();
        FeedFixture::wait_for_the_listing(&fixture);

        assert!(feed.collect().is_none());
        assert!(!feed.awaits_result());
    }
}
