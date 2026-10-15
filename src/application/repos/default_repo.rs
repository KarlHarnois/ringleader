use std::cmp::Reverse;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::application::Pending;
use crate::application::ports::{HostProbe, RepoScanner};
use crate::application::repo_merge::RepoMerge;
use crate::domain::{Repo, Session};

pub struct DefaultRepo {
    scanner: Arc<dyn RepoScanner>,
    host: Arc<dyn HostProbe>,
    launch_directory: PathBuf,
}

impl DefaultRepo {
    pub fn new(
        scanner: Arc<dyn RepoScanner>,
        host: Arc<dyn HostProbe>,
        launch_directory: &Path,
    ) -> Self {
        Self {
            scanner,
            host,
            launch_directory: launch_directory.to_path_buf(),
        }
    }

    pub fn find(&self, explicit: Vec<Repo>, sessions: &[Session]) -> Pending<Option<Repo>> {
        let scanner = Arc::clone(&self.scanner);
        let host = Arc::clone(&self.host);
        let launch_directory = self.launch_directory.clone();
        let recently_used = Self::repo_paths_newest_first(sessions);
        Pending::start(move || {
            let launch_repo = scanner
                .resolve_scope(&launch_directory)
                .ok()
                .and_then(|scope| scope.enclosing_repo().map(Path::to_path_buf));
            let mut still_present = recently_used
                .into_iter()
                .filter(|path| host.path_exists(path));
            let chosen = launch_repo.or_else(|| still_present.next());
            let merge = RepoMerge::new(host.as_ref());
            Ok(chosen.and_then(|top_level| merge.resolved(&top_level, &explicit).ok()))
        })
    }

    fn repo_paths_newest_first(sessions: &[Session]) -> Vec<PathBuf> {
        let mut newest_first: Vec<&Session> = sessions.iter().collect();
        newest_first.sort_by_key(|session| Reverse(session.created_at()));
        newest_first
            .iter()
            .map(|session| session.repo_path().to_path_buf())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::domain::{Config, RepoName};
    use crate::test_support::{FakeHostProbe, FakeRepoScanner, ServiceFixture};

    struct DefaultFixture;

    impl DefaultFixture {
        const WAIT_LIMIT: Duration = Duration::from_secs(2);
        const OLDER: &'static str = "/work/older-project";
        const NEWER: &'static str = "/work/newer-project";

        fn chosen(fixture: &ServiceFixture) -> Option<Repo> {
            let pending = fixture.service().default_repo();
            let started = Instant::now();
            pending
                .wait_unless(|| started.elapsed() > Self::WAIT_LIMIT)
                .expect("the default repo resolved in time")
                .unwrap()
        }

        fn launched_inside_the_repo() -> ServiceFixture {
            ServiceFixture::new().with_scanner(
                FakeRepoScanner::finding(Vec::new()).inside_repo(Path::new(ServiceFixture::REPO)),
            )
        }

        fn with_two_sessions(present: &[&str]) -> ServiceFixture {
            let host = present.iter().fold(FakeHostProbe::default(), |host, path| {
                host.with_directory(Path::new(path))
            });
            ServiceFixture::new()
                .with_sessions(&[
                    ServiceFixture::session_at("older", Self::OLDER, 100),
                    ServiceFixture::session_at("newer", Self::NEWER, 200),
                ])
                .with_host(host)
        }
    }

    #[test]
    fn a_launch_inside_a_repo_chooses_that_repo() {
        let chosen = DefaultFixture::chosen(&DefaultFixture::launched_inside_the_repo()).unwrap();

        assert_eq!(chosen.name().as_str(), "example-project");
        assert_eq!(chosen.path(), Path::new(ServiceFixture::REPO));
    }

    #[test]
    fn a_launch_inside_a_configured_repo_chooses_its_entry() {
        let entry = Repo::explicit(
            RepoName::parse("custom-name").unwrap(),
            Path::new(ServiceFixture::REPO),
            None,
        );
        let config: Config = ServiceFixture::config().with_repos(vec![entry]);
        let fixture = DefaultFixture::launched_inside_the_repo().with_config(config);

        let chosen = DefaultFixture::chosen(&fixture).unwrap();

        assert_eq!(chosen.name().as_str(), "custom-name");
        assert!(chosen.is_explicit());
    }

    #[test]
    fn a_launch_outside_any_repo_chooses_the_newest_session_repo() {
        let fixture =
            DefaultFixture::with_two_sessions(&[DefaultFixture::OLDER, DefaultFixture::NEWER]);

        let chosen = DefaultFixture::chosen(&fixture).unwrap();

        assert_eq!(chosen.path(), Path::new(DefaultFixture::NEWER));
    }

    #[test]
    fn a_session_repo_that_is_gone_is_skipped() {
        let fixture = DefaultFixture::with_two_sessions(&[DefaultFixture::OLDER]);

        let chosen = DefaultFixture::chosen(&fixture).unwrap();

        assert_eq!(chosen.path(), Path::new(DefaultFixture::OLDER));
    }

    #[test]
    fn no_launch_repo_and_no_sessions_chooses_nothing() {
        assert_eq!(DefaultFixture::chosen(&ServiceFixture::new()), None);
    }
}
