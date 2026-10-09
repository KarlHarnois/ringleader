use super::{CliSession, CommandStop, RepoAddArgs};
use crate::application::{AddRepoRequest, RepoAddition, ServiceError, SessionService};
use crate::domain::BaseBranch;

pub struct RepoCommands<'a> {
    service: &'a mut SessionService,
    session: &'a mut CliSession,
}

impl<'a> RepoCommands<'a> {
    pub fn new(service: &'a mut SessionService, session: &'a mut CliSession) -> Self {
        Self { service, session }
    }

    pub fn add(&mut self, args: &RepoAddArgs) -> Result<u8, CommandStop> {
        let path = self
            .session
            .wait(&self.service.read_typed_path(&args.path))?;
        let mut request = AddRepoRequest::new(&path);
        if let Some(name) = &args.name {
            request = request.named(name);
        }
        if let Some(base) = args.base_branch.as_deref().and_then(BaseBranch::new) {
            request = request.based_on(base);
        }
        self.session.stop_if_signaled()?;
        match self.session.wait(&self.service.add_repo(request))? {
            RepoAddition::Added(repo) => {
                self.session.output().line(&format!(
                    "added {} for {}",
                    repo.name(),
                    repo.path().display()
                ));
                Ok(0)
            }
            RepoAddition::AlreadyConfigured(repo) => {
                self.session
                    .output()
                    .line(&format!("already configured as {}", repo.name()));
                Ok(0)
            }
            RepoAddition::NameTaken { name, taken_by } => Err(ServiceError::Invalid(format!(
                "the name {name} is used by the [[repos]] entry for {}, pass another --name",
                taken_by.path().display()
            ))
            .into()),
        }
    }

    pub fn remove(&mut self, name: &str) -> Result<u8, CommandStop> {
        self.session.stop_if_signaled()?;
        let removed = self.session.wait(&self.service.remove_repo(name))?;
        self.session
            .output()
            .line(&format!("removed the [[repos]] entry {removed}"));
        Ok(0)
    }

    pub fn list(&mut self) -> Result<u8, CommandStop> {
        let discovery = self.session.wait(&self.service.discover_repos())?;
        for repo in discovery.repos() {
            let marker = if repo.is_explicit() {
                "  [[repos]]"
            } else {
                ""
            };
            self.session.output().line(&format!(
                "{:<24} {}{marker}",
                repo.name(),
                repo.path().display()
            ));
        }
        for notice in discovery.notices() {
            self.session.output().error(&notice);
        }
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::cli::CliExit;
    use crate::domain::{Repo, RepoName};
    use crate::test_support::{CliHarness, FakeRepoScanner, FakeWorkspace, ServiceFixture};

    struct RepoFixture;

    impl RepoFixture {
        const OTHER: &'static str = "/work/other-project";

        fn with_other_repo() -> ServiceFixture {
            ServiceFixture::new()
                .with_host(
                    ServiceFixture::host_for(&[])
                        .with_directory(Path::new(Self::OTHER))
                        .with_directory(Path::new("/work/other-project/src")),
                )
                .with_workspace(
                    FakeWorkspace::default().with_top_level(
                        Path::new("/work/other-project/src"),
                        Path::new(Self::OTHER),
                    ),
                )
        }

        fn explicit(name: &str, path: &str) -> Repo {
            Repo::explicit(RepoName::parse(name).unwrap(), Path::new(path), None)
        }
    }

    #[test]
    fn repo_add_pins_a_repo_under_its_basename() {
        let harness = CliHarness::new(RepoFixture::with_other_repo());

        assert_eq!(
            harness.run(&["repo", "add", RepoFixture::OTHER]),
            CliExit::Code(0)
        );
        let repos = harness.fixture().config_source().config().repos().to_vec();
        assert_eq!(repos[0].name().as_str(), "other-project");
    }

    #[test]
    fn adding_the_same_repo_again_or_through_a_subdirectory_leaves_one_entry() {
        let harness = CliHarness::new(RepoFixture::with_other_repo());

        harness.run(&["repo", "add", RepoFixture::OTHER]);
        harness.run(&["repo", "add", RepoFixture::OTHER]);
        harness.run(&["repo", "add", "/work/other-project/src"]);

        assert_eq!(harness.fixture().config_source().config().repos().len(), 1);
        assert!(
            harness
                .stdout()
                .contains("already configured as other-project")
        );
    }

    #[test]
    fn a_differing_name_for_a_configured_path_exits_three_and_writes_nothing() {
        let config = ServiceFixture::config()
            .with_repos(vec![RepoFixture::explicit("pinned", RepoFixture::OTHER)]);
        let harness = CliHarness::new(RepoFixture::with_other_repo().with_config(config));

        let exit = harness.run(&["repo", "add", RepoFixture::OTHER, "--name", "renamed"]);

        assert_eq!(exit, CliExit::Code(3));
        assert_eq!(harness.fixture().config_source().write_count(), 0);
    }

    #[test]
    fn a_name_another_entry_uses_exits_one_and_writes_nothing() {
        let config = ServiceFixture::config().with_repos(vec![RepoFixture::explicit(
            "other-project",
            "/elsewhere/other-project",
        )]);
        let harness = CliHarness::new(RepoFixture::with_other_repo().with_config(config));

        let exit = harness.run(&["repo", "add", RepoFixture::OTHER]);

        assert_eq!(exit, CliExit::Code(1));
        assert!(harness.stderr().contains("/elsewhere/other-project"));
        assert_eq!(harness.fixture().config_source().write_count(), 0);
    }

    #[test]
    fn repo_remove_drops_an_unused_override() {
        let config = ServiceFixture::config()
            .with_repos(vec![RepoFixture::explicit("pinned", RepoFixture::OTHER)]);
        let harness = CliHarness::new(ServiceFixture::new().with_config(config));

        assert_eq!(harness.run(&["repo", "remove", "pinned"]), CliExit::Code(0));
        assert!(
            harness
                .fixture()
                .config_source()
                .config()
                .repos()
                .is_empty()
        );
    }

    #[test]
    fn repo_remove_refuses_while_a_session_uses_the_repo() {
        let config = ServiceFixture::config()
            .with_repos(vec![RepoFixture::explicit("pinned", ServiceFixture::REPO)]);
        let harness = CliHarness::new(
            ServiceFixture::new()
                .with_config(config)
                .with_sessions(&[ServiceFixture::session("fix-auth")]),
        );

        let exit = harness.run(&["repo", "remove", "pinned"]);

        assert_eq!(exit, CliExit::Code(3));
        assert!(harness.stderr().contains("fix-auth"));
    }

    #[test]
    fn repo_remove_ignores_a_session_of_another_repo_with_the_same_name() {
        let config = ServiceFixture::config().with_repos(vec![RepoFixture::explicit(
            "example-project",
            "/elsewhere/example-project",
        )]);
        let harness = CliHarness::new(
            ServiceFixture::new()
                .with_config(config)
                .with_sessions(&[ServiceFixture::session("fix-auth")]),
        );

        assert_eq!(
            harness.run(&["repo", "remove", "example-project"]),
            CliExit::Code(0)
        );
    }

    #[test]
    fn removing_a_repo_found_only_by_scanning_says_so() {
        let harness = CliHarness::new(ServiceFixture::new());

        let exit = harness.run(&["repo", "remove", "example-project"]);

        assert_eq!(exit, CliExit::Code(2));
        assert!(harness.stderr().contains("found by scanning"));
    }

    #[test]
    fn repo_list_marks_an_explicit_override_outside_the_scope() {
        let config = ServiceFixture::config()
            .with_repos(vec![RepoFixture::explicit("pinned", "/opt/pinned")]);
        let fixture = ServiceFixture::new()
            .with_config(config)
            .with_scanner(FakeRepoScanner::finding(vec![ServiceFixture::repo()]));
        let harness = CliHarness::new(fixture);

        harness.run(&["repo", "list"]);

        let stdout = harness.stdout();
        assert!(stdout.contains("example-project"));
        assert!(
            stdout
                .lines()
                .any(|line| line.contains("/opt/pinned") && line.ends_with("[[repos]]"))
        );
    }
}
