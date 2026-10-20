use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::application::Pending;
use crate::application::ports::{HostProbe, RepoScanner};
use crate::application::repo_merge::RepoMerge;
use crate::domain::Repo;

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

    pub fn find(
        &self,
        explicit: Vec<Repo>,
        preferred_repo: Option<PathBuf>,
    ) -> Pending<Option<Repo>> {
        let scanner = Arc::clone(&self.scanner);
        let host = Arc::clone(&self.host);
        let launch_directory = self.launch_directory.clone();
        Pending::start(move || {
            let chosen = preferred_repo
                .filter(|path| host.path_exists(path))
                .or_else(|| Self::launch_repo(scanner.as_ref(), &launch_directory));
            let merge = RepoMerge::new(host.as_ref());
            Ok(chosen.and_then(|top_level| merge.resolved(&top_level, &explicit).ok()))
        })
    }

    fn launch_repo(scanner: &dyn RepoScanner, launch_directory: &Path) -> Option<PathBuf> {
        scanner
            .resolve_scope(launch_directory)
            .ok()
            .and_then(|scope| scope.enclosing_repo().map(Path::to_path_buf))
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
        const PREFERRED: &'static str = "/work/preferred-project";

        fn chosen(fixture: &ServiceFixture) -> Option<Repo> {
            Self::chosen_for(fixture, None)
        }

        fn chosen_for_the_preferred_repo(fixture: &ServiceFixture) -> Option<Repo> {
            Self::chosen_for(fixture, Some(PathBuf::from(Self::PREFERRED)))
        }

        fn chosen_for(fixture: &ServiceFixture, preferred_repo: Option<PathBuf>) -> Option<Repo> {
            let pending = fixture.service().default_repo(preferred_repo);
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

        fn with_the_preferred_repo_present(fixture: ServiceFixture) -> ServiceFixture {
            fixture.with_host(FakeHostProbe::default().with_directory(Path::new(Self::PREFERRED)))
        }
    }

    #[test]
    fn a_preferred_repo_wins_over_the_launch_repo() {
        let fixture = DefaultFixture::with_the_preferred_repo_present(
            DefaultFixture::launched_inside_the_repo(),
        );

        let chosen = DefaultFixture::chosen_for_the_preferred_repo(&fixture).unwrap();

        assert_eq!(chosen.path(), Path::new(DefaultFixture::PREFERRED));
    }

    #[test]
    fn a_preferred_repo_that_is_gone_falls_back_to_the_launch_repo() {
        let fixture = DefaultFixture::launched_inside_the_repo();

        let chosen = DefaultFixture::chosen_for_the_preferred_repo(&fixture).unwrap();

        assert_eq!(chosen.path(), Path::new(ServiceFixture::REPO));
    }

    #[test]
    fn a_preferred_configured_repo_chooses_its_entry() {
        let entry = Repo::explicit(
            RepoName::parse("custom-name").unwrap(),
            Path::new(DefaultFixture::PREFERRED),
            None,
        );
        let config: Config = ServiceFixture::config().with_repos(vec![entry]);
        let fixture = DefaultFixture::with_the_preferred_repo_present(
            ServiceFixture::new().with_config(config),
        );

        let chosen = DefaultFixture::chosen_for_the_preferred_repo(&fixture).unwrap();

        assert_eq!(chosen.name().as_str(), "custom-name");
        assert!(chosen.is_explicit());
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
    fn no_preferred_repo_and_no_launch_repo_chooses_nothing() {
        assert_eq!(DefaultFixture::chosen(&ServiceFixture::new()), None);
    }
}
