use std::time::Instant;

use crate::application::{Pending, SessionService};
use crate::domain::{LoadSample, SessionLoad, SessionSummary};

#[derive(Default)]
pub struct ResourceFeed {
    previous: Option<LoadSample>,
    reading: ResourceReading,
    pending: Option<Pending<LoadSample>>,
    next_sample: Option<Instant>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ResourceReading {
    #[default]
    Measuring,
    Loads(Vec<SessionLoad>),
    Failed,
}

impl ResourceFeed {
    pub fn reading(&self) -> &ResourceReading {
        &self.reading
    }

    #[cfg(test)]
    pub fn is_sampling(&self) -> bool {
        self.pending.is_some()
    }

    pub fn poll(&mut self, now: Instant, service: &SessionService, rows: &[SessionSummary]) {
        self.collect();
        if self.pending.is_none() && self.is_due(now) {
            self.pending = Some(service.sample_load(rows));
            self.next_sample = Some(now + LoadSample::INTERVAL);
        }
    }

    fn is_due(&self, now: Instant) -> bool {
        self.next_sample.is_none_or(|next| now >= next)
    }

    fn collect(&mut self) {
        let Some(finished) = self.pending.as_ref().and_then(Pending::poll) else {
            return;
        };
        self.pending = None;
        match finished {
            Ok(sample) => self.show(sample),
            Err(_) => self.reading = ResourceReading::Failed,
        }
    }

    fn show(&mut self, sample: LoadSample) {
        if let Some(previous) = &self.previous {
            self.reading = ResourceReading::Loads(sample.loads_since(previous));
        }
        self.previous = Some(sample);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::domain::{ProcessFigures, ProcessSample, SessionState};
    use crate::test_support::{
        Deadline, FakeMultiplexer, FakeProcessMeter, HeldCalls, LogCapture, ServiceFixture,
    };

    struct FeedFixture;

    impl FeedFixture {
        const PANE: u32 = 100;

        fn row() -> SessionSummary {
            SessionSummary::managed(
                ServiceFixture::session("fix-auth"),
                SessionState::Working,
                false,
            )
        }

        fn pane_spent(seconds: u64, cpu_milliseconds: u64) -> ProcessSample {
            let pane = ProcessFigures::new(Self::PANE, 1, "claude")
                .with_cpu_time(Duration::from_millis(cpu_milliseconds));
            ProcessSample::new(Duration::from_secs(seconds), vec![pane])
        }

        fn sampling(samples: Vec<ProcessSample>) -> ServiceFixture {
            let tmux_name = ServiceFixture::session("fix-auth").tmux_name().to_string();
            ServiceFixture::new()
                .with_multiplexer(FakeMultiplexer::new().with_pane_pids(&tmux_name, &[Self::PANE]))
                .with_process_meter(FakeProcessMeter::new().with_samples(samples))
        }

        fn settle(feed: &mut ResourceFeed, now: Instant, service: &SessionService) {
            let rows = [Self::row()];
            feed.poll(now, service, &rows);
            let settled = Deadline::for_background_work().wait_until(|| {
                feed.poll(now, service, &rows);
                !feed.is_sampling()
            });
            assert!(settled, "the sample never finished");
        }

        fn cpu(feed: &ResourceFeed) -> Option<String> {
            match feed.reading() {
                ResourceReading::Loads(loads) => Some(loads[0].cpu().to_string()),
                ResourceReading::Measuring | ResourceReading::Failed => None,
            }
        }
    }

    #[test]
    fn the_first_poll_starts_a_sample() {
        let fixture = FeedFixture::sampling(Vec::new());
        let mut feed = ResourceFeed::default();

        feed.poll(Instant::now(), &fixture.service(), &[FeedFixture::row()]);

        assert!(feed.is_sampling());
    }

    #[test]
    fn one_sample_alone_measures_no_cpu_yet() {
        let fixture = FeedFixture::sampling(vec![FeedFixture::pane_spent(0, 0)]);
        let service = fixture.service();
        let mut feed = ResourceFeed::default();

        FeedFixture::settle(&mut feed, Instant::now(), &service);

        assert_eq!(feed.reading(), &ResourceReading::Measuring);
    }

    #[test]
    fn the_feed_waits_a_second_before_sampling_again() {
        let fixture = FeedFixture::sampling(Vec::new());
        let service = fixture.service();
        let mut feed = ResourceFeed::default();
        let start = Instant::now();
        FeedFixture::settle(&mut feed, start, &service);

        feed.poll(
            start + LoadSample::INTERVAL - Duration::from_millis(1),
            &service,
            &[FeedFixture::row()],
        );

        assert!(!feed.is_sampling());
    }

    #[test]
    fn the_feed_samples_again_once_a_second_passed() {
        let fixture = FeedFixture::sampling(Vec::new());
        let service = fixture.service();
        let mut feed = ResourceFeed::default();
        let start = Instant::now();
        FeedFixture::settle(&mut feed, start, &service);

        feed.poll(
            start + LoadSample::INTERVAL,
            &service,
            &[FeedFixture::row()],
        );

        assert!(feed.is_sampling());
    }

    #[test]
    fn each_sample_after_the_first_refreshes_the_loads() {
        let fixture = FeedFixture::sampling(vec![
            FeedFixture::pane_spent(0, 0),
            FeedFixture::pane_spent(1, 200),
            FeedFixture::pane_spent(2, 900),
        ]);
        let service = fixture.service();
        let mut feed = ResourceFeed::default();
        let start = Instant::now();
        FeedFixture::settle(&mut feed, start, &service);
        FeedFixture::settle(&mut feed, start + LoadSample::INTERVAL, &service);
        let after_the_second = FeedFixture::cpu(&feed);

        FeedFixture::settle(&mut feed, start + LoadSample::INTERVAL * 2, &service);

        assert_eq!(after_the_second.as_deref(), Some("20.0%"));
        assert_eq!(FeedFixture::cpu(&feed).as_deref(), Some("70.0%"));
    }

    #[test]
    fn a_sample_in_flight_does_not_hold_up_the_poll() {
        let held = HeldCalls::all();
        let fixture = FeedFixture::sampling(Vec::new());
        fixture.process_meter().on_sample(held.hook());
        let service = fixture.service();
        let mut feed = ResourceFeed::default();
        let rows = [FeedFixture::row()];
        feed.poll(Instant::now(), &service, &rows);
        held.wait_for(1);

        feed.poll(Instant::now() + LoadSample::INTERVAL, &service, &rows);

        assert!(feed.is_sampling());
        held.release();
    }

    #[test]
    fn an_unreadable_process_table_shows_no_loads_and_is_logged() {
        let log = LogCapture::installed();
        let fixture = ServiceFixture::new()
            .with_process_meter(FakeProcessMeter::new().unreadable("example denial of the feed"));
        let service = fixture.service();
        let mut feed = ResourceFeed::default();

        FeedFixture::settle(&mut feed, Instant::now(), &service);

        assert_eq!(feed.reading(), &ResourceReading::Failed);
        assert_eq!(log.count_containing("example denial of the feed"), 1);
    }
}
