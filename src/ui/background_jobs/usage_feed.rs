use std::time::{Duration, Instant};

use crate::application::{Pending, SessionService, UsageReading};

pub struct UsageFeed {
    reading: Option<UsageReading>,
    pending: Option<Pending<UsageReading>>,
    next_measurement: Option<Instant>,
}

impl UsageFeed {
    pub const REFRESH: Duration = Duration::from_secs(300);

    pub fn open(service: &SessionService) -> Self {
        Self {
            reading: service.last_usage(),
            pending: None,
            next_measurement: None,
        }
    }

    pub fn reading(&self) -> Option<&UsageReading> {
        self.reading.as_ref()
    }

    pub fn is_measuring(&self) -> bool {
        self.pending.is_some()
    }

    pub fn poll(&mut self, now: Instant, service: &SessionService) {
        self.collect(now);
        if self.is_due(now) {
            self.pending = Some(service.measure_usage());
        }
    }

    fn collect(&mut self, now: Instant) {
        let Some(finished) = self.pending.as_ref().and_then(Pending::poll) else {
            return;
        };
        self.pending = None;
        self.next_measurement = Some(now + Self::REFRESH);
        match finished {
            Ok(reading) => self.reading = Some(reading),
            Err(error) => log::warn!("could not measure the agents' usage: {error}"),
        }
    }

    fn is_due(&self, now: Instant) -> bool {
        self.pending.is_none() && self.next_measurement.is_none_or(|due| now >= due)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AgentUsage, LimitWindow, UsageLimit, UsageRow};
    use crate::test_support::{Deadline, FakeUsageMeter, ServiceFixture};

    struct FeedFixture;

    impl FeedFixture {
        fn measuring_claude() -> ServiceFixture {
            let limit = UsageLimit::used(LimitWindow::Session, 42.0, None);
            let usage = AgentUsage::new(vec![UsageRow::Limit(limit)]);
            ServiceFixture::new()
                .with_usage_meter(FakeUsageMeter::new().with_usage("claude", usage))
        }

        fn settle(feed: &mut UsageFeed, now: Instant, service: &SessionService) {
            feed.poll(now, service);
            let settled = Deadline::for_background_work().wait_until(|| {
                feed.poll(now, service);
                !feed.is_measuring()
            });
            assert!(settled, "the measurement never finished");
        }

        fn measurements(fixture: &ServiceFixture) -> usize {
            fixture.usage_meter().calls().len()
        }
    }

    #[test]
    fn opening_shows_the_last_reading_at_once() {
        let fixture = FeedFixture::measuring_claude();
        let service = fixture.service();
        let earlier = service
            .measure_usage()
            .wait_unless(|| false)
            .unwrap()
            .unwrap();

        let feed = UsageFeed::open(&service);

        assert_eq!(feed.reading(), Some(&earlier));
    }

    #[test]
    fn the_first_poll_starts_a_measurement() {
        let fixture = FeedFixture::measuring_claude();
        let service = fixture.service();
        let mut feed = UsageFeed::open(&service);

        FeedFixture::settle(&mut feed, Instant::now(), &service);

        assert_eq!(feed.reading().unwrap().agents()[0].agent(), "claude");
    }

    #[test]
    fn the_feed_waits_for_the_refresh_before_measuring_again() {
        let fixture = FeedFixture::measuring_claude();
        let service = fixture.service();
        let mut feed = UsageFeed::open(&service);
        let start = Instant::now();
        FeedFixture::settle(&mut feed, start, &service);

        feed.poll(
            start + UsageFeed::REFRESH - Duration::from_secs(1),
            &service,
        );

        assert!(!feed.is_measuring());
        assert_eq!(FeedFixture::measurements(&fixture), 1);
    }

    #[test]
    fn the_feed_measures_again_once_the_refresh_passed() {
        let fixture = FeedFixture::measuring_claude();
        let service = fixture.service();
        let mut feed = UsageFeed::open(&service);
        let start = Instant::now();
        FeedFixture::settle(&mut feed, start, &service);

        FeedFixture::settle(&mut feed, start + UsageFeed::REFRESH, &service);

        assert_eq!(FeedFixture::measurements(&fixture), 2);
    }
}
