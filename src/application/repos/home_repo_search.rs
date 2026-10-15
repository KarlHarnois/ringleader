use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::repo_catalog::RepoDiscovery;
use crate::application::Pending;
use crate::application::ServiceError;
use crate::application::ports::{HostProbe, RepoScanner};
use crate::application::repo_merge::RepoMerge;
use crate::domain::{Repo, RepoScope};

pub struct HomeRepoSearch {
    scanner: Arc<dyn RepoScanner>,
    host: Arc<dyn HostProbe>,
    home: Option<PathBuf>,
}

impl HomeRepoSearch {
    pub const DEPTH: usize = 5;
    const NO_HOME: &'static str = "there is no home directory to search, type a path instead";

    pub fn new(
        scanner: Arc<dyn RepoScanner>,
        host: Arc<dyn HostProbe>,
        home: Option<&Path>,
    ) -> Self {
        Self {
            scanner,
            host,
            home: home.map(Path::to_path_buf),
        }
    }

    pub fn find(&self, explicit: Vec<Repo>) -> Pending<RepoDiscovery> {
        let scanner = Arc::clone(&self.scanner);
        let host = Arc::clone(&self.host);
        let home = self.home.clone();
        Pending::start(move || {
            let home = home.ok_or_else(|| ServiceError::Invalid(Self::NO_HOME.to_string()))?;
            let scope = RepoScope::new(&home);
            let outcome = scanner
                .scan(&scope, Self::DEPTH)
                .map_err(|error| ServiceError::Invalid(error.to_string()))?;
            let mut repos = RepoMerge::new(host.as_ref()).merge(&scope, outcome.repos(), &explicit);
            repos.sort_by(|first, second| first.path().cmp(second.path()));
            Ok(RepoDiscovery::new(scope, repos, outcome.cut_short()))
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::domain::{Config, RepoName};
    use crate::test_support::{FakeRepoScanner, ServiceFixture};

    struct SearchFixture;

    impl SearchFixture {
        const WAIT_LIMIT: Duration = Duration::from_secs(2);

        fn repo(name: &str, path: &str) -> Repo {
            Repo::discovered(RepoName::parse(name).unwrap(), Path::new(path))
        }

        fn found(fixture: &ServiceFixture) -> Result<RepoDiscovery, ServiceError> {
            let pending = fixture.service().find_repos();
            let started = Instant::now();
            pending
                .wait_unless(|| started.elapsed() > Self::WAIT_LIMIT)
                .expect("the search finished in time")
        }

        fn paths(discovery: &RepoDiscovery) -> Vec<&Path> {
            discovery.repos().iter().map(Repo::path).collect()
        }
    }

    #[test]
    fn the_search_scans_the_home_directory() {
        let fixture = ServiceFixture::new();

        SearchFixture::found(&fixture).unwrap();

        assert_eq!(
            fixture.scanner().scans(),
            [(PathBuf::from(ServiceFixture::HOME), HomeRepoSearch::DEPTH)]
        );
    }

    #[test]
    fn found_repos_are_sorted_by_path() {
        let fixture = ServiceFixture::new().with_scanner(FakeRepoScanner::finding(vec![
            SearchFixture::repo("zeta", "/home/example/zeta"),
            SearchFixture::repo("alpha", "/home/example/work/alpha"),
            SearchFixture::repo("beta", "/home/example/beta"),
        ]));

        let found = SearchFixture::found(&fixture).unwrap();

        assert_eq!(
            SearchFixture::paths(&found),
            [
                Path::new("/home/example/beta"),
                Path::new("/home/example/work/alpha"),
                Path::new("/home/example/zeta"),
            ]
        );
    }

    #[test]
    fn a_configured_entry_replaces_the_scanned_repo_at_its_path() {
        let entry = Repo::explicit(
            RepoName::parse("custom-name").unwrap(),
            Path::new("/home/example/beta"),
            None,
        );
        let config: Config = ServiceFixture::config().with_repos(vec![entry]);
        let fixture =
            ServiceFixture::new()
                .with_config(config)
                .with_scanner(FakeRepoScanner::finding(vec![
                    SearchFixture::repo("beta", "/home/example/beta"),
                    SearchFixture::repo("alpha", "/home/example/alpha"),
                ]));

        let found = SearchFixture::found(&fixture).unwrap();

        let names: Vec<&str> = found
            .repos()
            .iter()
            .map(|repo| repo.name().as_str())
            .collect();
        assert_eq!(names, ["alpha", "custom-name"]);
    }

    #[test]
    fn a_search_without_a_home_directory_fails() {
        let fixture = ServiceFixture::new().without_home();

        let error = SearchFixture::found(&fixture).unwrap_err();

        assert!(error.to_string().contains("no home directory to search"));
    }

    #[test]
    fn a_search_cut_short_keeps_the_notice() {
        let fixture = ServiceFixture::new()
            .with_scanner(FakeRepoScanner::finding(vec![ServiceFixture::repo()]).cut_short());

        let found = SearchFixture::found(&fixture).unwrap();

        assert_eq!(found.notices(), ["the scan of /home/example was cut short"]);
    }
}
