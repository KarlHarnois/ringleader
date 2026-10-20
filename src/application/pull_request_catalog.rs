use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::pending::Pending;
use super::ports::{PullRequestError, PullRequests};
use crate::domain::{PullRequest, Session};

type BranchListing = HashMap<String, Option<PullRequest>>;

#[derive(Clone)]
pub struct PullRequestCatalog {
    pull_requests: Arc<dyn PullRequests>,
    failing_repos: Arc<Mutex<HashSet<PathBuf>>>,
    listings: Arc<Mutex<HashMap<PathBuf, BranchListing>>>,
}

impl PullRequestCatalog {
    pub fn new(pull_requests: Arc<dyn PullRequests>) -> Self {
        Self {
            pull_requests,
            failing_repos: Arc::default(),
            listings: Arc::default(),
        }
    }

    pub fn find(&self, sessions: &[&Session]) -> Pending<HashMap<String, Option<PullRequest>>> {
        let catalog = self.clone();
        let queries: Vec<PullRequestQuery> = sessions
            .iter()
            .filter_map(|session| PullRequestQuery::of_worktree(session))
            .collect();
        Pending::start(move || Ok(catalog.fetch(queries)))
    }

    pub fn latest(&self, session: &Session) -> Option<PullRequest> {
        let query = PullRequestQuery::of_worktree(session)?;
        let listings = self.listings();
        let listed = listings.get(&query.repo)?;
        let (_, pull_request) = query.answer(listed)?;
        pull_request
    }

    pub fn look_up_unless_known(&self, session: &Session) -> Result<(), PullRequestError> {
        match (self.latest(session), PullRequestQuery::of_worktree(session)) {
            (None, Some(query)) => self.look_up(query),
            _ => Ok(()),
        }
    }

    fn look_up(&self, query: PullRequestQuery) -> Result<(), PullRequestError> {
        let listed = self.pull_requests.lookup(&query.repo, &[query.branch])?;
        self.remember(query.repo, listed);
        Ok(())
    }

    fn fetch(&self, queries: Vec<PullRequestQuery>) -> HashMap<String, Option<PullRequest>> {
        let mut found = HashMap::new();
        for (repo, group) in Self::by_repo(queries) {
            let branches: Vec<String> = group.iter().map(|query| query.branch.clone()).collect();
            match self.pull_requests.lookup(&repo, &branches) {
                Ok(listed) => {
                    self.failing_repos().remove(&repo);
                    found.extend(group.into_iter().filter_map(|query| query.answer(&listed)));
                    self.remember(repo, listed);
                }
                Err(error) => self.warn_once(&repo, &error),
            }
        }
        found
    }

    fn remember(&self, repo: PathBuf, listed: BranchListing) {
        self.listings().entry(repo).or_default().extend(listed);
    }

    fn warn_once(&self, repo: &Path, error: &PullRequestError) {
        if self.failing_repos().insert(repo.to_path_buf()) {
            log::warn!(
                "could not look up the pull requests in {}: {error}",
                repo.display()
            );
        }
    }

    fn failing_repos(&self) -> MutexGuard<'_, HashSet<PathBuf>> {
        self.failing_repos
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn listings(&self) -> MutexGuard<'_, HashMap<PathBuf, BranchListing>> {
        self.listings.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn by_repo(queries: Vec<PullRequestQuery>) -> BTreeMap<PathBuf, Vec<PullRequestQuery>> {
        let mut by_repo: BTreeMap<PathBuf, Vec<PullRequestQuery>> = BTreeMap::new();
        for query in queries {
            by_repo.entry(query.repo.clone()).or_default().push(query);
        }
        by_repo
    }
}

struct PullRequestQuery {
    name: String,
    repo: PathBuf,
    branch: String,
    created_at: u64,
}

impl PullRequestQuery {
    fn of_worktree(session: &Session) -> Option<Self> {
        if session.runs_in_main_checkout() {
            return None;
        }
        Some(Self {
            name: session.name().to_string(),
            repo: session.repo_path().to_path_buf(),
            branch: session.branch().to_string(),
            created_at: session.created_at(),
        })
    }

    fn answer(self, listed: &BranchListing) -> Option<(String, Option<PullRequest>)> {
        let newest = listed.get(&self.branch)?.as_ref();
        let own = newest.filter(|pull_request| pull_request.opened_since(self.created_at));
        Some((self.name, own.cloned()))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::domain::PullRequestState;
    use crate::test_support::{FakePullRequests, FakeWorkspace, LogCapture, ServiceFixture};

    struct CatalogFixture;

    impl CatalogFixture {
        fn open(number: u64) -> PullRequest {
            ServiceFixture::pull_request(number, PullRequestState::Open)
        }

        fn with_pull_request(session: &Session, number: u64) -> Arc<FakePullRequests> {
            Arc::new(FakePullRequests::new().with_pull_request(
                Path::new(ServiceFixture::REPO),
                session.branch(),
                Self::open(number),
            ))
        }

        fn found(
            fake: Arc<FakePullRequests>,
            sessions: &[&Session],
        ) -> HashMap<String, Option<PullRequest>> {
            Self::settle(&PullRequestCatalog::new(fake), sessions)
        }

        fn settle(
            catalog: &PullRequestCatalog,
            sessions: &[&Session],
        ) -> HashMap<String, Option<PullRequest>> {
            catalog
                .find(sessions)
                .wait_unless(|| false)
                .unwrap()
                .unwrap()
        }

        fn looked_up(fake: Arc<FakePullRequests>, session: &Session) -> PullRequestCatalog {
            let catalog = PullRequestCatalog::new(fake);
            Self::settle(&catalog, &[session]);
            catalog
        }
    }

    #[test]
    fn a_lookup_finds_the_pull_request_of_a_session() {
        let session = ServiceFixture::session("fix-auth");
        let fake = CatalogFixture::with_pull_request(&session, 142);

        let found = CatalogFixture::found(fake, &[&session]);

        assert_eq!(
            found.get("fix-auth"),
            Some(&Some(CatalogFixture::open(142)))
        );
    }

    #[test]
    fn a_branch_without_a_pull_request_is_found_as_none() {
        let session = ServiceFixture::session("fix-auth");

        let found = CatalogFixture::found(Arc::new(FakePullRequests::new()), &[&session]);

        assert_eq!(found.get("fix-auth"), Some(&None));
    }

    #[test]
    fn a_session_in_the_main_checkout_is_never_looked_up() {
        let session = ServiceFixture::main_checkout_session("fix-auth");
        let fake = Arc::new(FakePullRequests::new());

        CatalogFixture::found(fake.clone(), &[&session]);

        assert!(fake.calls().is_empty());
    }

    #[test]
    fn sessions_of_one_repo_share_one_lookup() {
        let first = ServiceFixture::session("fix-auth");
        let second = ServiceFixture::session("add-search");
        let fake = Arc::new(FakePullRequests::new());

        CatalogFixture::found(fake.clone(), &[&first, &second]);

        assert_eq!(
            fake.calls(),
            vec![format!("lookup {} 2", ServiceFixture::REPO)]
        );
    }

    #[test]
    fn a_pull_request_opened_before_the_session_is_not_its_own() {
        let session = ServiceFixture::session("fix-auth");
        let older = PullRequest::new(
            1600,
            PullRequestState::Merged,
            ServiceFixture::CREATED_AT - 1,
            &ServiceFixture::pull_request_url(1600),
        );
        let fake = Arc::new(FakePullRequests::new().with_pull_request(
            Path::new(ServiceFixture::REPO),
            session.branch(),
            older,
        ));

        let found = CatalogFixture::found(fake, &[&session]);

        assert_eq!(found.get("fix-auth"), Some(&None));
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

        let found = CatalogFixture::found(fake, &[&reachable, &unreachable]);

        assert_eq!(
            found.get("fix-auth"),
            Some(&Some(CatalogFixture::open(142)))
        );
        assert_eq!(found.get("local"), None);
    }

    #[test]
    fn a_repo_that_keeps_failing_is_warned_about_once() {
        let log = LogCapture::installed();
        let session = ServiceFixture::session_at(
            "fix-auth",
            "/repos/always-failing",
            ServiceFixture::CREATED_AT,
        );
        let fake = Arc::new(FakePullRequests::new());
        fake.fail();
        let catalog = PullRequestCatalog::new(fake);

        for _ in 0..3 {
            CatalogFixture::settle(&catalog, &[&session]);
        }

        assert_eq!(log.count_containing("/repos/always-failing"), 1);
    }

    #[test]
    fn a_repo_that_fails_again_after_recovering_is_warned_about_again() {
        let log = LogCapture::installed();
        let session = ServiceFixture::session_at(
            "fix-auth",
            "/repos/failing-again",
            ServiceFixture::CREATED_AT,
        );
        let fake = Arc::new(FakePullRequests::new());
        let catalog = PullRequestCatalog::new(fake.clone());

        fake.fail();
        CatalogFixture::settle(&catalog, &[&session]);
        fake.recover();
        CatalogFixture::settle(&catalog, &[&session]);
        fake.fail();
        CatalogFixture::settle(&catalog, &[&session]);

        assert_eq!(log.count_containing("/repos/failing-again"), 2);
    }

    #[test]
    fn the_latest_pull_request_of_a_session_is_the_one_its_lookup_found() {
        let session = ServiceFixture::session("fix-auth");
        let fake = CatalogFixture::with_pull_request(&session, 142);

        let catalog = CatalogFixture::looked_up(fake, &session);

        assert_eq!(catalog.latest(&session), Some(CatalogFixture::open(142)));
    }

    #[test]
    fn a_session_never_looked_up_has_no_latest_pull_request() {
        let session = ServiceFixture::session("fix-auth");
        let catalog = PullRequestCatalog::new(CatalogFixture::with_pull_request(&session, 142));

        assert_eq!(catalog.latest(&session), None);
    }

    #[test]
    fn a_main_checkout_session_has_no_latest_pull_request_even_when_its_branch_has_one() {
        let worktree = ServiceFixture::session_on_branch("release", FakeWorkspace::DEFAULT_BRANCH);
        let fake = CatalogFixture::with_pull_request(&worktree, 142);

        let catalog = CatalogFixture::looked_up(fake, &worktree);

        let main_checkout = ServiceFixture::main_checkout_session("fix-auth");
        assert_eq!(catalog.latest(&main_checkout), None);
    }

    #[test]
    fn a_session_made_after_the_pull_request_opened_does_not_take_it_as_its_latest() {
        let session = ServiceFixture::session("fix-auth");
        let fake = CatalogFixture::with_pull_request(&session, 142);
        let catalog = CatalogFixture::looked_up(fake, &session);

        let recreated =
            ServiceFixture::session_at("fix-auth", ServiceFixture::REPO, ServiceFixture::NOW + 1);

        assert_eq!(catalog.latest(&recreated), None);
    }

    #[test]
    fn a_pull_request_opened_since_the_last_lookup_is_found_on_demand() {
        let session = ServiceFixture::session("fix-auth");
        let fake = Arc::new(FakePullRequests::new());
        let catalog = CatalogFixture::looked_up(fake.clone(), &session);

        fake.add_pull_request(
            Path::new(ServiceFixture::REPO),
            session.branch(),
            CatalogFixture::open(142),
        );
        catalog.look_up_unless_known(&session).unwrap();

        assert_eq!(catalog.latest(&session), Some(CatalogFixture::open(142)));
    }

    #[test]
    fn a_known_pull_request_is_not_looked_up_again() {
        let session = ServiceFixture::session("fix-auth");
        let fake = CatalogFixture::with_pull_request(&session, 142);
        let catalog = CatalogFixture::looked_up(fake.clone(), &session);

        catalog.look_up_unless_known(&session).unwrap();

        assert_eq!(fake.calls().len(), 1);
    }

    #[test]
    fn a_failed_on_demand_lookup_is_an_error() {
        let session = ServiceFixture::session("fix-auth");
        let fake = Arc::new(FakePullRequests::new());
        fake.fail();
        let catalog = PullRequestCatalog::new(fake);

        let looked_up = catalog.look_up_unless_known(&session);

        assert!(matches!(looked_up, Err(PullRequestError::Failed { .. })));
    }

    #[test]
    fn a_main_checkout_session_is_never_looked_up_on_demand() {
        let session = ServiceFixture::main_checkout_session("fix-auth");
        let fake = Arc::new(FakePullRequests::new());
        let catalog = PullRequestCatalog::new(fake.clone());

        catalog.look_up_unless_known(&session).unwrap();

        assert!(fake.calls().is_empty());
    }

    #[test]
    fn a_failed_lookup_keeps_the_pull_request_last_found() {
        let session = ServiceFixture::session("fix-auth");
        let fake = CatalogFixture::with_pull_request(&session, 142);
        let catalog = CatalogFixture::looked_up(fake.clone(), &session);

        fake.fail();
        CatalogFixture::settle(&catalog, &[&session]);

        assert_eq!(catalog.latest(&session), Some(CatalogFixture::open(142)));
    }

    #[test]
    fn a_pull_request_that_is_gone_from_the_listing_is_no_longer_the_latest() {
        let session = ServiceFixture::session("fix-auth");
        let fake = CatalogFixture::with_pull_request(&session, 142);
        let catalog = CatalogFixture::looked_up(fake.clone(), &session);

        fake.remove_pull_request(Path::new(ServiceFixture::REPO), session.branch());
        CatalogFixture::settle(&catalog, &[&session]);

        assert_eq!(catalog.latest(&session), None);
    }

    #[test]
    fn a_lookup_of_another_session_keeps_the_latest_pull_request_of_the_first() {
        let first = ServiceFixture::session("fix-auth");
        let second = ServiceFixture::session("add-search");
        let fake = CatalogFixture::with_pull_request(&first, 142);
        let catalog = CatalogFixture::looked_up(fake, &first);

        CatalogFixture::settle(&catalog, &[&second]);

        assert_eq!(catalog.latest(&first), Some(CatalogFixture::open(142)));
    }
}
