use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::pending::Pending;
use super::ports::{PullRequestError, PullRequests};
use crate::domain::{PullRequest, Session};

#[derive(Clone)]
pub struct PullRequestCatalog {
    pull_requests: Arc<dyn PullRequests>,
    failing_repos: Arc<Mutex<HashSet<PathBuf>>>,
}

impl PullRequestCatalog {
    pub fn new(pull_requests: Arc<dyn PullRequests>) -> Self {
        Self {
            pull_requests,
            failing_repos: Arc::default(),
        }
    }

    pub fn find(&self, sessions: &[&Session]) -> Pending<HashMap<String, Option<PullRequest>>> {
        let catalog = self.clone();
        let queries: Vec<PullRequestQuery> = sessions
            .iter()
            .map(|session| PullRequestQuery::of(session))
            .collect();
        Pending::start(move || Ok(catalog.fetch(queries)))
    }

    fn fetch(&self, queries: Vec<PullRequestQuery>) -> HashMap<String, Option<PullRequest>> {
        let mut found = HashMap::new();
        for (repo, group) in Self::by_repo(queries) {
            let branches: Vec<String> = group.iter().map(|query| query.branch.clone()).collect();
            match self.pull_requests.lookup(&repo, &branches) {
                Ok(listed) => {
                    self.failing_repos().remove(&repo);
                    found.extend(group.into_iter().filter_map(|query| query.answer(&listed)));
                }
                Err(error) => self.warn_once(&repo, &error),
            }
        }
        found
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
    fn of(session: &Session) -> Self {
        Self {
            name: session.name().to_string(),
            repo: session.repo_path().to_path_buf(),
            branch: session.branch().to_string(),
            created_at: session.created_at(),
        }
    }

    fn answer(
        self,
        listed: &HashMap<String, Option<PullRequest>>,
    ) -> Option<(String, Option<PullRequest>)> {
        let newest = *listed.get(&self.branch)?;
        let own = newest.filter(|pull_request| pull_request.opened_since(self.created_at));
        Some((self.name, own))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::domain::PullRequestState;
    use crate::test_support::{FakePullRequests, LogCapture, ServiceFixture};

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
}
