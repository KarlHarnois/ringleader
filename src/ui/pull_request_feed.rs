use std::collections::HashMap;
use std::time::Instant;

use crate::application::{Pending, SessionService};
use crate::domain::PullRequest;

#[derive(Default)]
pub struct PullRequestFeed {
    pull_requests: HashMap<String, PullRequest>,
    pending: Option<Pending<HashMap<String, Option<PullRequest>>>>,
    next_refresh: Option<Instant>,
    refresh_requested: bool,
}

impl PullRequestFeed {
    pub fn pull_requests(&self) -> &HashMap<String, PullRequest> {
        &self.pull_requests
    }

    pub fn refresh_soon(&mut self) {
        self.refresh_requested = true;
    }

    pub fn keep_only(&mut self, names: &[String]) {
        self.pull_requests.retain(|name, _| names.contains(name));
    }

    pub fn tick(&mut self, now: Instant, visible: &[String], service: &SessionService) {
        let next_refresh = now + service.config().intervals().pull_request_refresh();
        self.collect(next_refresh);
        if self.is_due(now) && !visible.is_empty() {
            self.start(next_refresh, visible, service);
        }
    }

    fn collect(&mut self, next_refresh: Instant) {
        let Some(finished) = self.pending.as_ref().and_then(Pending::poll) else {
            return;
        };
        self.pending = None;
        self.next_refresh = Some(next_refresh);
        match finished {
            Ok(found) => self.merge(found),
            Err(error) => log::warn!("could not look up pull requests: {error}"),
        }
    }

    fn is_due(&self, now: Instant) -> bool {
        let scheduled = self.refresh_requested || self.next_refresh.is_none_or(|due| now >= due);
        self.pending.is_none() && scheduled
    }

    fn start(&mut self, next_refresh: Instant, visible: &[String], service: &SessionService) {
        self.next_refresh = Some(next_refresh);
        self.refresh_requested = false;
        self.pending = Some(service.find_pull_requests(visible));
    }

    fn merge(&mut self, found: HashMap<String, Option<PullRequest>>) {
        for (name, pull_request) in found {
            match pull_request {
                Some(pull_request) => {
                    self.pull_requests.insert(name, pull_request);
                }
                None => {
                    self.pull_requests.remove(&name);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use super::*;
    use crate::domain::PullRequestState;
    use crate::test_support::{FakePullRequests, ServiceFixture};

    struct FeedFixture;

    impl FeedFixture {
        const NAME: &'static str = "fix-auth";
        const LIMIT: Duration = Duration::from_secs(2);

        fn open(number: u64) -> PullRequest {
            PullRequest::new(number, PullRequestState::Open)
        }

        fn with_pull_request(number: u64) -> ServiceFixture {
            let session = ServiceFixture::session(Self::NAME);
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&session))
                .with_pull_requests(FakePullRequests::new().with_pull_request(
                    Path::new(ServiceFixture::REPO),
                    session.branch(),
                    Self::open(number),
                ))
        }

        fn without_pull_request() -> ServiceFixture {
            ServiceFixture::new().with_sessions(&[ServiceFixture::session(Self::NAME)])
        }

        fn visible() -> Vec<String> {
            vec![Self::NAME.to_string()]
        }

        fn settle(feed: &mut PullRequestFeed, fixture: &ServiceFixture, service: &SessionService) {
            let recorded = fixture.pull_requests().calls().len();
            let deadline = Instant::now() + Self::LIMIT;
            while Instant::now() < deadline {
                feed.tick(Instant::now(), &Self::visible(), service);
                let looked_up = fixture.pull_requests().calls().len() > recorded;
                if looked_up && feed.pending.is_none() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    #[test]
    fn a_tick_caches_the_pull_request_of_a_visible_session() {
        let fixture = FeedFixture::with_pull_request(142);
        let service = fixture.service();
        let mut feed = PullRequestFeed::default();

        FeedFixture::settle(&mut feed, &fixture, &service);

        assert_eq!(
            feed.pull_requests().get(FeedFixture::NAME),
            Some(&FeedFixture::open(142))
        );
    }

    #[test]
    fn a_branch_without_a_pull_request_is_left_out_of_the_cache() {
        let fixture = FeedFixture::without_pull_request();
        let service = fixture.service();
        let mut feed = PullRequestFeed::default();

        FeedFixture::settle(&mut feed, &fixture, &service);

        assert!(feed.pull_requests().is_empty());
    }

    #[test]
    fn a_failing_lookup_keeps_the_previous_cache() {
        let fixture = FeedFixture::with_pull_request(142);
        let service = fixture.service();
        let mut feed = PullRequestFeed::default();
        FeedFixture::settle(&mut feed, &fixture, &service);

        fixture.pull_requests().fail();
        feed.refresh_soon();
        FeedFixture::settle(&mut feed, &fixture, &service);

        assert_eq!(
            feed.pull_requests().get(FeedFixture::NAME),
            Some(&FeedFixture::open(142))
        );
    }

    #[test]
    fn a_refresh_requested_during_a_lookup_runs_as_soon_as_it_finishes() {
        let fixture = FeedFixture::with_pull_request(142);
        let service = fixture.service();
        let mut feed = PullRequestFeed::default();
        FeedFixture::settle(&mut feed, &fixture, &service);

        feed.refresh_soon();
        feed.tick(Instant::now(), &FeedFixture::visible(), &service);
        feed.refresh_soon();
        FeedFixture::settle(&mut feed, &fixture, &service);

        assert_eq!(fixture.pull_requests().calls().len(), 3);
    }

    #[test]
    fn pruning_drops_the_pull_requests_of_vanished_sessions() {
        let fixture = FeedFixture::with_pull_request(142);
        let service = fixture.service();
        let mut feed = PullRequestFeed::default();
        FeedFixture::settle(&mut feed, &fixture, &service);

        feed.keep_only(&["other".to_string()]);

        assert!(feed.pull_requests().is_empty());
    }
}
