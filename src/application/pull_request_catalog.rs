use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use super::pending::Pending;
use super::ports::PullRequests;
use crate::domain::{PullRequest, Session};

pub struct PullRequestCatalog {
    pull_requests: Arc<dyn PullRequests>,
}

impl PullRequestCatalog {
    pub fn new(pull_requests: Arc<dyn PullRequests>) -> Self {
        Self { pull_requests }
    }

    pub fn find(&self, sessions: &[&Session]) -> Pending<HashMap<String, Option<PullRequest>>> {
        let port = Arc::clone(&self.pull_requests);
        let queries: Vec<PullRequestQuery> = sessions
            .iter()
            .map(|session| PullRequestQuery::of(session))
            .collect();
        Pending::start(move || Ok(Self::fetch(port, queries)))
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
        fn open(number: u64) -> PullRequest {
            PullRequest::new(number, PullRequestState::Open)
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
            let pending = PullRequestCatalog::new(fake).find(sessions);
            pending.wait_unless(|| false).unwrap().unwrap()
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
}
