use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::thread;
use std::time::Duration;

use super::ports::{HostProbe, RepoScanner};
use super::service_context::ServiceContext;
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use crate::domain::{Config, Repo, RepoName, RepoScope};

pub struct RepoCatalog {
    scanner: Arc<dyn RepoScanner>,
    host: Arc<dyn HostProbe>,
    home: Option<PathBuf>,
    launch_directory: PathBuf,
}

impl RepoCatalog {
    pub fn new(ports: &ServicePorts, context: &ServiceContext) -> Self {
        Self {
            scanner: ports.repos().scanner(),
            host: ports.repos().host(),
            home: context.home().map(Path::to_path_buf),
            launch_directory: context.launch_directory().to_path_buf(),
        }
    }

    pub fn launch_directory(&self) -> &Path {
        &self.launch_directory
    }

    pub fn discover(&self, scope_directory: &Path, config: &Config) -> Pending<RepoDiscovery> {
        let scanner = Arc::clone(&self.scanner);
        let host = Arc::clone(&self.host);
        let scope_directory = scope_directory.to_path_buf();
        let explicit = config.repos().to_vec();
        let depth = config.scan_depth();
        Pending::start(move || {
            let scope = scanner
                .resolve_scope(&scope_directory)
                .map_err(|error| ServiceError::Invalid(error.to_string()))?;
            let outcome = scanner
                .scan(&scope, depth)
                .map_err(|error| ServiceError::Invalid(error.to_string()))?;
            let merged = RepoMerge::new(host.as_ref()).merge(&scope, outcome.repos(), &explicit);
            Ok(RepoDiscovery {
                scope,
                repos: merged,
                cut_short: outcome.cut_short(),
            })
        })
    }

    pub fn read_typed_path(&self, input: &str) -> Pending<PathBuf> {
        let host = Arc::clone(&self.host);
        let path = TypedPath::new(input, self.home.as_deref(), &self.launch_directory).expanded();
        Pending::start(move || {
            if !host.path_exists(&path) {
                return Err(ServiceError::Invalid(format!(
                    "no such directory: {}",
                    path.display()
                )));
            }
            if host.is_file(&path) {
                return Err(ServiceError::Invalid(format!(
                    "not a directory: {}",
                    path.display()
                )));
            }
            Ok(path)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoDiscovery {
    scope: RepoScope,
    repos: Vec<Repo>,
    cut_short: bool,
}

impl RepoDiscovery {
    pub fn scope(&self) -> &RepoScope {
        &self.scope
    }

    pub fn repos(&self) -> &[Repo] {
        &self.repos
    }

    pub fn cut_short(&self) -> bool {
        self.cut_short
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

pub struct Pending<T> {
    receiver: Receiver<Result<T, ServiceError>>,
}

impl<T: Send + 'static> Pending<T> {
    const POLL_INTERVAL: Duration = Duration::from_millis(100);

    pub fn start(work: impl FnOnce() -> Result<T, ServiceError> + Send + 'static) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(work());
        });
        Self { receiver }
    }

    pub fn poll(&self) -> Option<Result<T, ServiceError>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(Self::worker_failure())),
        }
    }

    pub fn wait_unless(&self, stop: impl Fn() -> bool) -> Option<Result<T, ServiceError>> {
        loop {
            match self.receiver.recv_timeout(Self::POLL_INTERVAL) {
                Ok(result) => return Some(result),
                Err(RecvTimeoutError::Disconnected) => return Some(Err(Self::worker_failure())),
                Err(RecvTimeoutError::Timeout) if stop() => return None,
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    fn worker_failure() -> ServiceError {
        ServiceError::Failed("the worker ended without a result".to_string())
    }
}

struct RepoMerge<'a> {
    host: &'a dyn HostProbe,
}

impl<'a> RepoMerge<'a> {
    fn new(host: &'a dyn HostProbe) -> Self {
        Self { host }
    }

    fn merge(&self, scope: &RepoScope, scanned: &[Repo], explicit: &[Repo]) -> Vec<Repo> {
        let mut merged: Vec<Repo> = explicit.iter().map(|repo| self.canonical(repo)).collect();
        let enclosing = scope.enclosing_repo().and_then(Self::discovered);
        for repo in enclosing.iter().chain(scanned) {
            if merged.iter().all(|kept| kept.path() != repo.path()) {
                merged.push(repo.clone());
            }
        }
        merged.sort_by(|first, second| {
            (first.name(), first.path()).cmp(&(second.name(), second.path()))
        });
        merged
    }

    fn canonical(&self, repo: &Repo) -> Repo {
        let canonical = self
            .host
            .canonicalize_lenient(repo.path())
            .unwrap_or_else(|_| repo.path().to_path_buf());
        repo.at_path(&canonical)
    }

    fn discovered(top_level: &Path) -> Option<Repo> {
        let basename = top_level.file_name()?.to_string_lossy().into_owned();
        Some(Repo::discovered(
            RepoName::from_basename(&basename).ok()?,
            top_level,
        ))
    }
}

struct TypedPath<'a> {
    input: &'a str,
    home: Option<&'a Path>,
    launch_directory: &'a Path,
}

impl<'a> TypedPath<'a> {
    fn new(input: &'a str, home: Option<&'a Path>, launch_directory: &'a Path) -> Self {
        Self {
            input,
            home,
            launch_directory,
        }
    }

    fn expanded(&self) -> PathBuf {
        let home_relative = match self.input {
            "~" => Some(""),
            input => input.strip_prefix("~/"),
        };
        match (home_relative, self.home) {
            (Some(rest), Some(home)) => home.join(rest),
            _ => self.launch_directory.join(self.input),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::application::ports::ScanError;
    use crate::domain::BaseBranch;
    use crate::test_support::{FakeMultiplexer, FakeRepoScanner, ServiceFixture};

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

        fn discover(fixture: &ServiceFixture, scope: &str) -> RepoDiscovery {
            let pending = fixture.service().discover_repos(Path::new(scope));
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

        fn typed(fixture: &ServiceFixture, input: &str) -> Result<PathBuf, ServiceError> {
            Self::settle(&fixture.service().read_typed_path(input))
        }
    }

    #[test]
    fn a_launch_inside_a_repo_lists_the_enclosing_repo() {
        let fixture = ServiceFixture::new().with_scanner(
            FakeRepoScanner::finding(Vec::new()).inside_repo(Path::new("/repos/example-project")),
        );

        let discovery = CatalogFixture::discover(&fixture, "/repos/example-project/src");

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

        let discovery = CatalogFixture::discover(&fixture, "/work");

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

        let discovery = CatalogFixture::discover(&fixture, "/work");

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

        let discovery = CatalogFixture::discover(&fixture, "/work");

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

        let discovery = CatalogFixture::discover(&fixture, "/work");

        assert_eq!(
            CatalogFixture::paths(&discovery),
            [
                PathBuf::from("/work/one/example-project"),
                PathBuf::from("/work/two/example-project")
            ]
        );
    }

    #[test]
    fn discovery_returns_while_a_scanner_still_blocks() {
        let fixture =
            ServiceFixture::new().with_scanner(FakeRepoScanner::finding(Vec::new()).blocked());

        let pending = fixture.service().discover_repos(Path::new("/work"));

        assert!(pending.poll().is_none());
        fixture.scanner().release();
        assert!(CatalogFixture::settle(&pending).is_ok());
    }

    #[test]
    fn a_cut_short_scan_says_so() {
        let fixture =
            ServiceFixture::new().with_scanner(FakeRepoScanner::finding(Vec::new()).cut_short());

        let discovery = CatalogFixture::discover(&fixture, "/work");

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

        let pending = fixture.service().discover_repos(Path::new("/work"));

        assert!(CatalogFixture::settle(&pending).is_err());
    }

    #[test]
    fn a_worker_that_ends_without_a_result_is_a_failed_scan() {
        let pending: Pending<()> = Pending::start(|| panic!("the scan worker failed"));

        assert!(CatalogFixture::settle(&pending).is_err());
    }

    #[test]
    fn sessions_are_listed_whatever_the_scope() {
        let elsewhere = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&elsewhere))
            .with_multiplexer(FakeMultiplexer::new());
        let mut service = fixture.service();
        CatalogFixture::settle(&service.discover_repos(Path::new("/unrelated"))).unwrap();

        let rows = service.observe(false).unwrap();

        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn a_tilde_expands_to_the_home_directory() {
        let fixture = ServiceFixture::new().with_host(
            ServiceFixture::host_for(&[]).with_directory(Path::new("/home/example/projects")),
        );

        assert_eq!(
            CatalogFixture::typed(&fixture, "~").unwrap(),
            Path::new("/home/example")
        );
        assert_eq!(
            CatalogFixture::typed(&fixture, "~/projects").unwrap(),
            Path::new("/home/example/projects")
        );
    }

    #[test]
    fn another_users_tilde_and_a_variable_stay_literal() {
        let fixture = ServiceFixture::new();

        let other_user = CatalogFixture::typed(&fixture, "~other/projects").unwrap_err();
        let variable = CatalogFixture::typed(&fixture, "$HOME/projects").unwrap_err();

        assert!(
            other_user
                .to_string()
                .contains("/repos/example-project/~other/projects")
        );
        assert!(
            variable
                .to_string()
                .contains("/repos/example-project/$HOME/projects")
        );
    }

    #[test]
    fn a_relative_path_joins_the_launch_directory() {
        let fixture = ServiceFixture::new().with_host(
            ServiceFixture::host_for(&[]).with_directory(Path::new("/repos/example-project/src")),
        );

        assert_eq!(
            CatalogFixture::typed(&fixture, "src").unwrap(),
            Path::new("/repos/example-project/src")
        );
    }

    #[test]
    fn a_file_is_refused_as_not_a_directory() {
        let fixture = ServiceFixture::new().with_host(
            ServiceFixture::host_for(&[]).with_file(Path::new("/repos/example-project/README.md")),
        );

        let error = CatalogFixture::typed(&fixture, "README.md").unwrap_err();

        assert_eq!(
            error.to_string(),
            "not a directory: /repos/example-project/README.md"
        );
    }

    #[test]
    fn a_missing_directory_is_refused() {
        let fixture = ServiceFixture::new();

        let error = CatalogFixture::typed(&fixture, "/nowhere").unwrap_err();

        assert_eq!(error.to_string(), "no such directory: /nowhere");
    }
}
