use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::pending::Pending;
use super::ports::PullRequests;
use crate::domain::{PullRequest, Session};

pub struct PullRequestCatalog {
    pull_requests: Option<Arc<dyn PullRequests>>,
    cached: HashMap<String, PullRequest>,
    pending: Option<Pending<HashMap<String, Option<PullRequest>>>>,
    next_refresh: Option<Instant>,
}

impl PullRequestCatalog {
    pub fn new(pull_requests: Option<Arc<dyn PullRequests>>) -> Self {
        Self {
            pull_requests,
            cached: HashMap::new(),
            pending: None,
            next_refresh: None,
        }
    }

    pub fn poll(
        &mut self,
        now: Instant,
        interval: Duration,
        visible: &[Session],
    ) -> &HashMap<String, PullRequest> {
        self.collect(now, interval);
        let due = self.next_refresh.is_none_or(|due| now >= due);
        if self.pull_requests.is_some() && self.pending.is_none() && !visible.is_empty() && due {
            self.start(now, interval, visible);
        }
        &self.cached
    }

    pub fn snapshot(&self) -> &HashMap<String, PullRequest> {
        &self.cached
    }

    pub fn keep_only(&mut self, names: &[String]) {
        self.cached.retain(|name, _| names.contains(name));
    }

    pub fn refresh_soon(&mut self) {
        self.next_refresh = None;
    }

    fn collect(&mut self, now: Instant, interval: Duration) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        match pending.poll() {
            None => self.pending = Some(pending),
            Some(Ok(found)) => {
                self.next_refresh = Some(now + interval);
                self.merge(found);
            }
            Some(Err(error)) => {
                log::warn!("could not look up pull requests: {error}");
                self.next_refresh = Some(now + interval);
            }
        }
    }

    fn start(&mut self, now: Instant, interval: Duration, visible: &[Session]) {
        let Some(port) = self.pull_requests.clone() else {
            return;
        };
        let queries: Vec<PullRequestQuery> = visible.iter().map(PullRequestQuery::of).collect();
        self.next_refresh = Some(now + interval);
        self.pending = Some(Pending::start(move || Ok(Self::fetch(port, queries))));
    }

    fn fetch(
        port: Arc<dyn PullRequests>,
        queries: Vec<PullRequestQuery>,
    ) -> HashMap<String, Option<PullRequest>> {
        let mut by_repo: BTreeMap<PathBuf, Vec<PullRequestQuery>> = BTreeMap::new();
        for query in queries {
            by_repo.entry(query.repo.clone()).or_default().push(query);
        }
        let mut found = HashMap::new();
        for (repo, group) in by_repo {
            let branches: Vec<String> = group.iter().map(|query| query.branch.clone()).collect();
            let listed = port.lookup(&repo, &branches);
            for query in group {
                match listed.get(&query.branch) {
                    Some(Ok(pull_request)) => {
                        found.insert(query.name, *pull_request);
                    }
                    Some(Err(error)) => log::warn!(
                        "could not look up the pull request of {} in {}: {error}",
                        query.branch,
                        repo.display()
                    ),
                    None => {}
                }
            }
        }
        found
    }

    fn merge(&mut self, found: HashMap<String, Option<PullRequest>>) {
        for (name, pull_request) in found {
            match pull_request {
                Some(pull_request) => {
                    self.cached.insert(name, pull_request);
                }
                None => {
                    self.cached.remove(&name);
                }
            }
        }
    }
}

struct PullRequestQuery {
    name: String,
    repo: PathBuf,
    branch: String,
}

impl PullRequestQuery {
    fn of(session: &Session) -> Self {
        Self {
            name: session.name().to_string(),
            repo: session.repo_path().to_path_buf(),
            branch: session.branch().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::domain::PullRequestState;
    use crate::test_support::{FakePullRequests, ServiceFixture};

    struct CatalogFixture;

    impl CatalogFixture {
        const INTERVAL: Duration = Duration::from_secs(30);
        const LIMIT: Duration = Duration::from_secs(2);

        fn open(number: u64) -> PullRequest {
            PullRequest::new(number, PullRequestState::Open)
        }

        fn cached(session: &Session, number: u64) -> Arc<FakePullRequests> {
            Arc::new(FakePullRequests::new().with_pull_request(
                Path::new(ServiceFixture::REPO),
                session.branch(),
                Self::open(number),
            ))
        }

        fn settle(
            catalog: &mut PullRequestCatalog,
            fake: &FakePullRequests,
            sessions: &[Session],
        ) -> HashMap<String, PullRequest> {
            let deadline = Instant::now() + Self::LIMIT;
            while Instant::now() < deadline {
                catalog.poll(Instant::now(), Self::INTERVAL, sessions);
                if !fake.calls().is_empty() && catalog.pending.is_none() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            catalog.snapshot().clone()
        }
    }

    #[test]
    fn a_refresh_caches_the_pull_request_of_a_visible_session() {
        let session = ServiceFixture::session("fix-auth");
        let fake = CatalogFixture::cached(&session, 142);
        let mut catalog = PullRequestCatalog::new(Some(fake.clone()));

        let found =
            CatalogFixture::settle(&mut catalog, fake.as_ref(), std::slice::from_ref(&session));

        assert_eq!(found.get("fix-auth"), Some(&CatalogFixture::open(142)));
    }

    #[test]
    fn a_branch_without_a_pull_request_is_left_out_of_the_cache() {
        let session = ServiceFixture::session("fix-auth");
        let fake = Arc::new(FakePullRequests::new());
        let mut catalog = PullRequestCatalog::new(Some(fake.clone()));

        let found =
            CatalogFixture::settle(&mut catalog, fake.as_ref(), std::slice::from_ref(&session));

        assert!(found.is_empty());
    }

    #[test]
    fn no_capability_means_no_lookups_and_no_rows() {
        let session = ServiceFixture::session("fix-auth");
        let mut catalog = PullRequestCatalog::new(None);

        let found = catalog.poll(Instant::now(), CatalogFixture::INTERVAL, &[session]);

        assert!(found.is_empty());
    }

    #[test]
    fn a_failing_lookup_keeps_the_previous_cache() {
        let session = ServiceFixture::session("fix-auth");
        let fake = CatalogFixture::cached(&session, 142);
        let mut catalog = PullRequestCatalog::new(Some(fake.clone()));
        CatalogFixture::settle(&mut catalog, fake.as_ref(), std::slice::from_ref(&session));
        let recorded = fake.calls().len();

        fake.fail();
        catalog.refresh_soon();
        let deadline = Instant::now() + CatalogFixture::LIMIT;
        while Instant::now() < deadline {
            catalog.poll(
                Instant::now(),
                CatalogFixture::INTERVAL,
                std::slice::from_ref(&session),
            );
            if fake.calls().len() > recorded && catalog.pending.is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        assert_eq!(
            catalog.snapshot().get("fix-auth"),
            Some(&CatalogFixture::open(142))
        );
    }

    #[test]
    fn a_repo_that_fails_does_not_block_another_repos_lookup() {
        let reachable = ServiceFixture::session("fix-auth");
        let unreachable =
            ServiceFixture::session_at("local", "/repos/local-only", ServiceFixture::CREATED_AT);
        let fake = Arc::new(
            FakePullRequests::new()
                .with_pull_request(
                    Path::new(ServiceFixture::REPO),
                    reachable.branch(),
                    CatalogFixture::open(142),
                )
                .failing_repo(Path::new("/repos/local-only")),
        );
        let mut catalog = PullRequestCatalog::new(Some(fake.clone()));
        let sessions = [reachable.clone(), unreachable];

        let found = CatalogFixture::settle(&mut catalog, fake.as_ref(), &sessions);

        assert_eq!(found.get("fix-auth"), Some(&CatalogFixture::open(142)));
        assert_eq!(found.get("local"), None);
    }

    #[test]
    fn pruning_drops_the_pull_requests_of_vanished_sessions() {
        let session = ServiceFixture::session("fix-auth");
        let fake = CatalogFixture::cached(&session, 142);
        let mut catalog = PullRequestCatalog::new(Some(fake.clone()));
        CatalogFixture::settle(&mut catalog, fake.as_ref(), std::slice::from_ref(&session));

        catalog.keep_only(&["other".to_string()]);

        assert!(catalog.snapshot().is_empty());
    }
}
