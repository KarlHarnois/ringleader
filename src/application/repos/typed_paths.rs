use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::application::Pending;
use crate::application::ServiceContext;
use crate::application::ServiceError;
use crate::application::ServicePorts;
use crate::application::ports::{HostProbe, Workspace};
use crate::application::repo_merge::RepoMerge;
use crate::domain::Repo;

pub struct TypedPaths {
    workspace: Arc<dyn Workspace>,
    host: Arc<dyn HostProbe>,
    home: Option<PathBuf>,
    launch_directory: PathBuf,
}

impl TypedPaths {
    pub fn new(ports: &ServicePorts, context: &ServiceContext) -> Self {
        Self {
            workspace: ports.repos().workspace(),
            host: ports.repos().host(),
            home: context.home().map(Path::to_path_buf),
            launch_directory: context.launch_directory().to_path_buf(),
        }
    }

    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    pub fn read(&self, input: &str) -> Pending<PathBuf> {
        let host = Arc::clone(&self.host);
        let path = self.expanded(input);
        Pending::start(move || Self::existing_directory(host.as_ref(), path))
    }

    pub fn repo_at(&self, input: &str, explicit: Vec<Repo>) -> Pending<Repo> {
        let host = Arc::clone(&self.host);
        let workspace = Arc::clone(&self.workspace);
        let path = self.expanded(input);
        Pending::start(move || {
            let directory = Self::existing_directory(host.as_ref(), path)?;
            let top_level = workspace.validate_repo(&directory)?;
            Ok(RepoMerge::new(host.as_ref()).resolved(&top_level, &explicit)?)
        })
    }

    fn expanded(&self, input: &str) -> PathBuf {
        TypedPath::new(input, self.home.as_deref(), &self.launch_directory).expanded()
    }

    fn existing_directory(host: &dyn HostProbe, path: PathBuf) -> Result<PathBuf, ServiceError> {
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
    use std::time::{Duration, Instant};

    use super::*;
    use crate::domain::{BaseBranch, Config, RepoName};
    use crate::test_support::{FakeWorkspace, ServiceFixture};

    struct TypedFixture;

    impl TypedFixture {
        const WAIT_LIMIT: Duration = Duration::from_secs(2);

        fn settle<T: Send + 'static>(pending: &Pending<T>) -> Result<T, ServiceError> {
            let started = Instant::now();
            pending
                .wait_unless(|| started.elapsed() > Self::WAIT_LIMIT)
                .expect("the worker finished in time")
        }

        fn typed(fixture: &ServiceFixture, input: &str) -> Result<PathBuf, ServiceError> {
            Self::settle(&fixture.service().read_typed_path(input))
        }

        fn repo_at(fixture: &ServiceFixture, input: &str) -> Result<Repo, ServiceError> {
            Self::settle(&fixture.service().repo_at(input))
        }

        fn explicit(name: &str, path: &str) -> Repo {
            Repo::explicit(
                RepoName::parse(name).unwrap(),
                Path::new(path),
                BaseBranch::new("develop"),
            )
        }

        fn with_typed_directories() -> ServiceFixture {
            ServiceFixture::new()
                .with_host(
                    ServiceFixture::host_for(&[])
                        .with_directory(Path::new("/work/tools"))
                        .with_directory(Path::new("/work/plain")),
                )
                .with_workspace(
                    FakeWorkspace::default().with_invalid_repo(Path::new("/work/plain")),
                )
        }
    }

    #[test]
    fn a_tilde_expands_to_the_home_directory() {
        let fixture = ServiceFixture::new().with_host(
            ServiceFixture::host_for(&[]).with_directory(Path::new("/home/example/projects")),
        );

        assert_eq!(
            TypedFixture::typed(&fixture, "~").unwrap(),
            Path::new("/home/example")
        );
        assert_eq!(
            TypedFixture::typed(&fixture, "~/projects").unwrap(),
            Path::new("/home/example/projects")
        );
    }

    #[test]
    fn another_users_tilde_and_a_variable_stay_literal() {
        let fixture = ServiceFixture::new();

        let other_user = TypedFixture::typed(&fixture, "~other/projects").unwrap_err();
        let variable = TypedFixture::typed(&fixture, "$HOME/projects").unwrap_err();

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
            TypedFixture::typed(&fixture, "src").unwrap(),
            Path::new("/repos/example-project/src")
        );
    }

    #[test]
    fn a_file_is_refused_as_not_a_directory() {
        let fixture = ServiceFixture::new().with_host(
            ServiceFixture::host_for(&[]).with_file(Path::new("/repos/example-project/README.md")),
        );

        let error = TypedFixture::typed(&fixture, "README.md").unwrap_err();

        assert_eq!(
            error.to_string(),
            "not a directory: /repos/example-project/README.md"
        );
    }

    #[test]
    fn a_missing_directory_is_refused() {
        let fixture = ServiceFixture::new();

        let error = TypedFixture::typed(&fixture, "/nowhere").unwrap_err();

        assert_eq!(error.to_string(), "no such directory: /nowhere");
    }

    #[test]
    fn a_typed_repo_path_gives_a_repo_named_after_its_folder() {
        let fixture = TypedFixture::with_typed_directories();

        let repo = TypedFixture::repo_at(&fixture, "/work/tools").unwrap();

        assert_eq!(repo.name().as_str(), "tools");
        assert_eq!(repo.path(), Path::new("/work/tools"));
        assert!(!repo.is_explicit());
    }

    #[test]
    fn a_typed_configured_path_gives_its_entry() {
        let entry = TypedFixture::explicit("custom-tools", "/work/tools");
        let config: Config = ServiceFixture::config().with_repos(vec![entry]);
        let fixture = TypedFixture::with_typed_directories().with_config(config);

        let repo = TypedFixture::repo_at(&fixture, "/work/tools").unwrap();

        assert_eq!(repo.name().as_str(), "custom-tools");
        assert!(repo.is_explicit());
    }

    #[test]
    fn a_typed_plain_directory_is_refused_as_not_a_repo() {
        let fixture = TypedFixture::with_typed_directories();

        let error = TypedFixture::repo_at(&fixture, "/work/plain").unwrap_err();

        assert!(error.to_string().contains("not a git repository"));
    }

    #[test]
    fn a_typed_missing_path_is_refused() {
        let fixture = TypedFixture::with_typed_directories();

        let error = TypedFixture::repo_at(&fixture, "/work/nowhere").unwrap_err();

        assert_eq!(error.to_string(), "no such directory: /work/nowhere");
    }
}
