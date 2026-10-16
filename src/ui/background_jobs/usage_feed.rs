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
        let due = *self
            .next_measurement
            .get_or_insert_with(|| now + Self::freshness_left(self.reading.as_ref(), service));
        if self.pending.is_none() && now >= due {
            self.pending = Some(service.measure_usage());
        }
    }

    fn freshness_left(reading: Option<&UsageReading>, service: &SessionService) -> Duration {
        let Some(reading) = reading else {
            return Duration::ZERO;
        };
        let age = service
            .now_epoch_seconds()
            .saturating_sub(reading.measured_at());
        Self::REFRESH.saturating_sub(Duration::from_secs(age))
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AgentUsage, LimitWindow, UsageLimit, UsageRow};
    use crate::test_support::{Deadline, FakeUsageMeter, ServiceFixture};

    struct FeedFixture;

    impl FeedFixture {
        const MINUTE: u64 = 60;

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

        fn measure_earlier(service: &SessionService) {
            service
                .measure_usage()
                .wait_unless(|| false)
                .unwrap()
                .unwrap();
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
    fn a_fresh_reading_is_not_measured_again_before_its_refresh_is_up() {
        let fixture = FeedFixture::measuring_claude();
        let service = fixture.service();
        FeedFixture::measure_earlier(&service);
        fixture.clock().advance(FeedFixture::MINUTE);
        let mut feed = UsageFeed::open(&service);
        let start = Instant::now();

        feed.poll(start, &service);
        feed.poll(
            start + UsageFeed::REFRESH - Duration::from_secs(FeedFixture::MINUTE + 1),
            &service,
        );

        assert!(!feed.is_measuring());
    }

    #[test]
    fn a_fresh_reading_is_measured_again_once_its_refresh_is_up() {
        let fixture = FeedFixture::measuring_claude();
        let service = fixture.service();
        FeedFixture::measure_earlier(&service);
        fixture.clock().advance(FeedFixture::MINUTE);
        let mut feed = UsageFeed::open(&service);
        let start = Instant::now();
        feed.poll(start, &service);

        feed.poll(
            start + UsageFeed::REFRESH - Duration::from_secs(FeedFixture::MINUTE),
            &service,
        );

        assert!(feed.is_measuring());
    }

    #[test]
    fn a_stale_reading_is_measured_again_at_once() {
        let fixture = FeedFixture::measuring_claude();
        let service = fixture.service();
        FeedFixture::measure_earlier(&service);
        fixture.clock().advance(UsageFeed::REFRESH.as_secs());
        let mut feed = UsageFeed::open(&service);

        feed.poll(Instant::now(), &service);

        assert!(feed.is_measuring());
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
