use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::default_repo::DefaultRepo;
use super::home_repo_search::HomeRepoSearch;
use super::repo_overrides::AddRepoRequest;
use super::repo_overrides::{RepoAddition, RepoOverrides};
use super::typed_paths::TypedPaths;
use crate::application::Pending;
use crate::application::ServiceContext;
use crate::application::ServiceError;
use crate::application::ServicePorts;
use crate::application::ports::{HostProbe, RepoScanner};
use crate::application::repo_merge::RepoMerge;
use crate::domain::{Config, Repo, RepoScope, Session};

pub struct RepoCatalog {
    overrides: RepoOverrides,
    home_search: HomeRepoSearch,
    default_repo: DefaultRepo,
    typed_paths: TypedPaths,
    scanner: Arc<dyn RepoScanner>,
    host: Arc<dyn HostProbe>,
    launch_directory: PathBuf,
}

impl RepoCatalog {
    pub fn new(ports: &ServicePorts, context: &ServiceContext) -> Self {
        Self {
            overrides: RepoOverrides::new(ports),
            home_search: HomeRepoSearch::new(
                ports.repos().scanner(),
                ports.repos().host(),
                context.home(),
            ),
            default_repo: DefaultRepo::new(
                ports.repos().scanner(),
                ports.repos().host(),
                context.launch_directory(),
            ),
            typed_paths: TypedPaths::new(ports, context),
            scanner: ports.repos().scanner(),
            host: ports.repos().host(),
            launch_directory: context.launch_directory().to_path_buf(),
        }
    }

    pub fn home(&self) -> Option<&Path> {
        self.typed_paths.home()
    }

    pub fn discover(&self, config: &Config) -> Pending<RepoDiscovery> {
        let scanner = Arc::clone(&self.scanner);
        let host = Arc::clone(&self.host);
        let launch_directory = self.launch_directory.clone();
        let explicit = config.repos().to_vec();
        let depth = config.scan_depth();
        Pending::start(move || {
            let scope = scanner
                .resolve_scope(&launch_directory)
                .map_err(|error| ServiceError::Invalid(error.to_string()))?;
            let outcome = scanner
                .scan(&scope, depth)
                .map_err(|error| ServiceError::Invalid(error.to_string()))?;
            let merged = RepoMerge::new(host.as_ref()).merge(&scope, outcome.repos(), &explicit);
            Ok(RepoDiscovery::new(scope, merged, outcome.cut_short()))
        })
    }

    pub fn find_repos(&self, config: &Config) -> Pending<RepoDiscovery> {
        self.home_search.find(config.repos().to_vec())
    }

    pub fn default_repo(
        &self,
        config: &Config,
        preferred_repo: Option<PathBuf>,
    ) -> Pending<Option<Repo>> {
        self.default_repo
            .find(config.repos().to_vec(), preferred_repo)
    }

    pub fn add_repo(&self, request: AddRepoRequest, config: &Config) -> Pending<RepoAddition> {
        self.overrides.add(request, config)
    }

    pub fn remove_repo(&self, name: &str, config: &Config, entries: &[Session]) -> Pending<String> {
        self.overrides.remove(name, config, entries)
    }

    pub fn read_typed_path(&self, input: &str) -> Pending<PathBuf> {
        self.typed_paths.read(input)
    }

    pub fn repo_at(&self, input: &str, config: &Config) -> Pending<Repo> {
        self.typed_paths.repo_at(input, config.repos().to_vec())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoDiscovery {
    scope: RepoScope,
    repos: Vec<Repo>,
    cut_short: bool,
}

impl RepoDiscovery {
    pub(super) fn new(scope: RepoScope, repos: Vec<Repo>, cut_short: bool) -> Self {
        Self {
            scope,
            repos,
            cut_short,
        }
    }

    pub fn scope(&self) -> &RepoScope {
        &self.scope
    }

    pub fn repos(&self) -> &[Repo] {
        &self.repos
    }

    pub fn notices(&self) -> Vec<String> {
        let mut notices = Vec::new();
        if self.cut_short {
            notices.push(format!(
                "the scan of {} was cut short",
                self.scope.root().display()
            ));
        }
        if let Some(directory) = self.scope.unresolved_git_file() {
            notices.push(format!(
                "{} is a worktree whose main repository git cannot name",
                directory.display()
            ));
        }
        notices
    }
}
#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::application::pending::Pending;
    use crate::application::ports::ScanError;
    use crate::domain::{BaseBranch, RepoName};
    use crate::test_support::{FakeRepoScanner, ServiceFixture};

    struct CatalogFixture;

    impl CatalogFixture {
        const WAIT_LIMIT: Duration = Duration::from_secs(2);

        fn discovered(name: &str, path: &str) -> Repo {
            Repo::discovered(RepoName::parse(name).unwrap(), Path::new(path))
        }

        fn explicit(name: &str, path: &str) -> Repo {
            Repo::explicit(
                RepoName::parse(name).unwrap(),
                Path::new(path),
                BaseBranch::new("develop"),
            )
        }

        fn discover(fixture: &ServiceFixture) -> RepoDiscovery {
            let pending = fixture.service().discover_repos();
            Self::settle(&pending).unwrap()
        }

        fn settle<T: Send + 'static>(pending: &Pending<T>) -> Result<T, ServiceError> {
            let started = Instant::now();
            pending
                .wait_unless(|| started.elapsed() > Self::WAIT_LIMIT)
                .expect("the worker finished in time")
        }

        fn paths(discovery: &RepoDiscovery) -> Vec<PathBuf> {
            discovery
                .repos()
                .iter()
                .map(|repo| repo.path().to_path_buf())
                .collect()
        }
    }

    #[test]
    fn a_launch_inside_a_repo_lists_the_enclosing_repo() {
        let fixture = ServiceFixture::new().with_scanner(
            FakeRepoScanner::finding(Vec::new()).inside_repo(Path::new("/repos/example-project")),
        );

        let discovery = CatalogFixture::discover(&fixture);

        assert_eq!(
            CatalogFixture::paths(&discovery),
            [PathBuf::from("/repos/example-project")]
        );
    }

    #[test]
    fn a_launch_from_a_plain_directory_lists_only_the_scanned_repos() {
        let scanned = vec![CatalogFixture::discovered(
            "example-project",
            "/work/example-project",
        )];
        let fixture = ServiceFixture::new().with_scanner(FakeRepoScanner::finding(scanned));

        let discovery = CatalogFixture::discover(&fixture);

        assert_eq!(discovery.scope().enclosing_repo(), None);
        assert_eq!(
            CatalogFixture::paths(&discovery),
            [PathBuf::from("/work/example-project")]
        );
    }

    #[test]
    fn an_explicit_override_wins_on_its_canonical_path() {
        let scanned = vec![CatalogFixture::discovered(
            "example-project",
            "/work/example-project",
        )];
        let config = ServiceFixture::config().with_repos(vec![CatalogFixture::explicit(
            "pinned",
            "/link/example-project",
        )]);
        let fixture = ServiceFixture::new()
            .with_config(config)
            .with_scanner(FakeRepoScanner::finding(scanned))
            .with_host(ServiceFixture::host_for(&[]).with_canonical(
                Path::new("/link/example-project"),
                Path::new("/work/example-project"),
            ));

        let discovery = CatalogFixture::discover(&fixture);

        assert_eq!(discovery.repos().len(), 1);
        assert_eq!(discovery.repos()[0].name().as_str(), "pinned");
        assert!(discovery.repos()[0].is_explicit());
    }

    #[test]
    fn an_explicit_override_outside_the_scope_is_listed() {
        let config = ServiceFixture::config().with_repos(vec![CatalogFixture::explicit(
            "elsewhere",
            "/opt/elsewhere",
        )]);
        let fixture = ServiceFixture::new()
            .with_config(config)
            .with_scanner(FakeRepoScanner::finding(Vec::new()));

        let discovery = CatalogFixture::discover(&fixture);

        assert_eq!(
            CatalogFixture::paths(&discovery),
            [PathBuf::from("/opt/elsewhere")]
        );
    }

    #[test]
    fn two_repos_with_one_name_are_both_listed_by_path() {
        let scanned = vec![
            CatalogFixture::discovered("example-project", "/work/two/example-project"),
            CatalogFixture::discovered("example-project", "/work/one/example-project"),
        ];
        let fixture = ServiceFixture::new().with_scanner(FakeRepoScanner::finding(scanned));

        let discovery = CatalogFixture::discover(&fixture);

        assert_eq!(
            CatalogFixture::paths(&discovery),
            [
                PathBuf::from("/work/one/example-project"),
                PathBuf::from("/work/two/example-project")
            ]
        );
    }

    #[test]
    fn discovery_scans_the_launch_directory_at_the_configured_depth() {
        let fixture = ServiceFixture::new();

        CatalogFixture::discover(&fixture);

        assert_eq!(
            fixture.scanner().scans(),
            [(
                PathBuf::from(ServiceFixture::REPO),
                ServiceFixture::config().scan_depth()
            )]
        );
    }

    #[test]
    fn discovery_returns_while_a_scanner_still_blocks() {
        let fixture =
            ServiceFixture::new().with_scanner(FakeRepoScanner::finding(Vec::new()).blocked());

        let pending = fixture.service().discover_repos();

        assert!(pending.poll().is_none());
        fixture.scanner().release();
        assert!(CatalogFixture::settle(&pending).is_ok());
    }

    #[test]
    fn a_cut_short_scan_says_so() {
        let fixture =
            ServiceFixture::new().with_scanner(FakeRepoScanner::finding(Vec::new()).cut_short());

        let discovery = CatalogFixture::discover(&fixture);

        assert!(discovery.notices()[0].contains("cut short"));
    }

    #[test]
    fn an_unreadable_scope_fails_the_discovery() {
        let fixture = ServiceFixture::new().with_scanner(
            FakeRepoScanner::finding(Vec::new()).failing(ScanError::Unreadable {
                path: PathBuf::from("/work"),
                detail: "Permission denied".to_string(),
            }),
        );

        let pending = fixture.service().discover_repos();

        assert!(CatalogFixture::settle(&pending).is_err());
    }

    #[test]
    fn a_worker_that_ends_without_a_result_is_a_failed_scan() {
        let pending: Pending<()> = Pending::start(|| panic!("the scan worker failed"));

        assert!(CatalogFixture::settle(&pending).is_err());
    }
}
