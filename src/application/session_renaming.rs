use std::path::PathBuf;
use std::sync::Arc;

use super::agent_hooks::AgentHooks;
use super::branch_availability::BranchAvailability;
use super::branch_ownership::{BranchMutation, BranchOwnership};
use super::ports::{HostProbe, Multiplexer, SessionRegistry, Workspace};
use super::reconciliation::{ManagedRow, Reconciliation};
use super::rename_request::RenameRequest;
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use crate::domain::{Config, Session, SessionName};

pub struct SessionRenaming {
    ownership: BranchOwnership,
    branches: BranchAvailability,
    workspace: Arc<dyn Workspace>,
    multiplexer: Arc<dyn Multiplexer>,
    host: Arc<dyn HostProbe>,
    registry: Arc<dyn SessionRegistry>,
    hooks: AgentHooks,
}

impl SessionRenaming {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            ownership: BranchOwnership::new(ports),
            branches: BranchAvailability::new(ports),
            workspace: ports.repos().workspace(),
            multiplexer: ports.tmux().multiplexer(),
            host: ports.repos().host(),
            registry: ports.state().registry(),
            hooks: AgentHooks::new(ports),
        }
    }

    pub fn rename(
        &self,
        row: &ManagedRow,
        request: &RenameRequest,
        reconciliation: &Reconciliation,
        config: &Config,
    ) -> Result<Session, ServiceError> {
        let renamed = self.renamed_session(row.session(), request)?;
        self.check_rename_availability(row, &renamed, reconciliation, request.keeps_branch())?;
        if !request.keeps_branch() {
            self.ownership
                .refuse_unmarked(row.session(), BranchMutation::Rename)?;
        }
        let renaming = BranchRenaming::new(self, row.session(), &renamed);
        renaming.apply()?;
        if row.is_live() {
            self.rename_tmux_session(row, &renamed, &renaming)?;
        }
        let entries = reconciliation.entries_with(row.session().name().as_str(), renamed.clone());
        self.persist_rename(row, &entries, &renaming)?;
        self.tell_agent_its_name(row, &renamed, config);
        Ok(renamed)
    }

    fn renamed_session(
        &self,
        session: &Session,
        request: &RenameRequest,
    ) -> Result<Session, ServiceError> {
        let name = SessionName::new(request.new_name())?;
        let branch = if request.keeps_branch() {
            session.branch().to_string()
        } else {
            session.naming().branch_name(&name)
        };
        Ok(session.renamed(name, &branch))
    }

    fn check_rename_availability(
        &self,
        row: &ManagedRow,
        renamed: &Session,
        reconciliation: &Reconciliation,
        keep_branch: bool,
    ) -> Result<(), ServiceError> {
        let name_taken = reconciliation
            .entries()
            .iter()
            .any(|entry| entry.name() == renamed.name());
        if name_taken || reconciliation.is_live_tmux_name(renamed.tmux_name()) {
            return Err(ServiceError::Conflict(format!(
                "a session named {} already exists",
                renamed.name()
            )));
        }
        if keep_branch {
            return Ok(());
        }
        self.branches
            .refuse_taken(row.session().repo_path(), renamed.branch())
    }

    fn rename_tmux_session(
        &self,
        row: &ManagedRow,
        renamed: &Session,
        renaming: &BranchRenaming,
    ) -> Result<(), ServiceError> {
        let old = row.session().tmux_name();
        match self
            .multiplexer
            .rename_session(old, renamed.tmux_name(), renamed.metadata())
        {
            Ok(()) => Ok(()),
            Err(error) if error.is_timeout() => {
                Err(ServiceError::from(error).adding_context(&format!(
                    "the branch is now {}, nothing was recorded, run ringleader doctor",
                    renamed.branch()
                )))
            }
            Err(error) => Err(renaming.roll_back(error.into())),
        }
    }

    fn tell_agent_its_name(&self, row: &ManagedRow, renamed: &Session, config: &Config) {
        if row.state().runs_agent()
            && let Some(agent) = config.agent(renamed.agent())
        {
            self.hooks.after_rename(renamed, agent);
        }
    }

    fn persist_rename(
        &self,
        row: &ManagedRow,
        entries: &[Session],
        renaming: &BranchRenaming,
    ) -> Result<(), ServiceError> {
        let Err(error) = self.registry.save(entries) else {
            return Ok(());
        };
        let error = ServiceError::from(error);
        if row.is_live() {
            return Err(error.adding_context(
                "tmux already carries the new name, the next TUI run or mutating session command records it",
            ));
        }
        Err(renaming.roll_back(error))
    }
}
struct BranchRenaming<'a> {
    workspace: &'a dyn Workspace,
    location: PathBuf,
    old: &'a str,
    new: &'a str,
}

impl<'a> BranchRenaming<'a> {
    fn new(renaming: &'a SessionRenaming, session: &'a Session, renamed: &'a Session) -> Self {
        let worktree = session.worktree_path();
        let location = if renaming.host.path_exists(worktree) {
            worktree
        } else {
            session.repo_path()
        };
        Self {
            workspace: renaming.workspace.as_ref(),
            location: location.to_path_buf(),
            old: session.branch(),
            new: renamed.branch(),
        }
    }

    fn apply(&self) -> Result<(), ServiceError> {
        if self.old == self.new {
            return Ok(());
        }
        Ok(self
            .workspace
            .rename_branch(&self.location, self.old, self.new)?)
    }

    fn roll_back(&self, error: ServiceError) -> ServiceError {
        if self.old == self.new {
            return error;
        }
        match self
            .workspace
            .rename_branch(&self.location, self.new, self.old)
        {
            Ok(()) => error,
            Err(rollback) => error.adding_context(&format!(
                "renaming the branch back also failed ({rollback}), git now holds {}",
                self.new
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::application::ports::{BranchMarker, MultiplexerError};
    use crate::domain::Agent;
    use crate::test_support::{FakeMultiplexer, FakeSessionRegistry, ServiceFixture};

    struct RenameFixture;

    impl RenameFixture {
        fn live(names: &[&str]) -> ServiceFixture {
            let sessions: Vec<Session> = names
                .iter()
                .map(|name| ServiceFixture::session(name))
                .collect();
            let multiplexer = sessions
                .iter()
                .fold(FakeMultiplexer::new(), |multiplexer, session| {
                    multiplexer.with_live(ServiceFixture::waiting(session))
                });
            let references: Vec<&Session> = sessions.iter().collect();
            ServiceFixture::new()
                .with_registry(FakeSessionRegistry::with_sessions(sessions.clone()))
                .with_workspace(ServiceFixture::workspace_for(&references))
                .with_host(ServiceFixture::host_for(&references))
                .with_multiplexer(multiplexer)
        }

        fn unmarked(name: &str) -> ServiceFixture {
            let session = ServiceFixture::session(name);
            let repo = Path::new(ServiceFixture::REPO);
            Self::stopped(name).with_workspace(
                ServiceFixture::workspace_for(&[&session]).without_marker(repo, session.branch()),
            )
        }

        fn stopped(name: &str) -> ServiceFixture {
            let session = ServiceFixture::session(name);
            ServiceFixture::new()
                .with_registry(FakeSessionRegistry::with_sessions(vec![session.clone()]))
                .with_workspace(ServiceFixture::workspace_for(&[&session]))
                .with_host(ServiceFixture::host_for(&[&session]))
        }
    }

    #[test]
    fn rename_moves_the_branch_and_the_tmux_session() {
        let fixture = RenameFixture::live(&["fix-auth"]);

        let renamed = fixture
            .service()
            .rename_session("fix-auth", "fix-login", false)
            .unwrap();

        assert_eq!(renamed.tmux_name(), "ringleader-fix-login");
        assert!(
            fixture
                .workspace()
                .has_branch(Path::new(ServiceFixture::REPO), "fix-login")
        );
        assert_eq!(fixture.multiplexer().live_names(), ["ringleader-fix-login"]);
        assert_eq!(fixture.registry().sessions(), vec![renamed]);
    }

    #[test]
    fn a_running_agent_is_told_its_new_name() {
        let fixture =
            RenameFixture::live(&["fix-auth"]).with_config(ServiceFixture::config_with_input());

        fixture
            .service()
            .rename_session("fix-auth", "fix-login", false)
            .unwrap();

        assert_eq!(
            fixture.multiplexer().typed_if_ready("ringleader-fix-login"),
            ["/rename fix-login"]
        );
    }

    #[test]
    fn an_exited_agent_is_not_told_its_new_name() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = RenameFixture::stopped("fix-auth")
            .with_config(ServiceFixture::config_with_input())
            .with_multiplexer(FakeMultiplexer::new().with_live(ServiceFixture::exited(&session)));

        fixture
            .service()
            .rename_session("fix-auth", "fix-login", false)
            .unwrap();

        assert!(
            fixture
                .multiplexer()
                .typed_if_ready("ringleader-fix-login")
                .is_empty()
        );
    }

    #[test]
    fn an_agent_no_longer_configured_is_not_told_its_new_name() {
        let another_agent =
            Agent::new("opencode", "opencode").with_rename_input(&["/rename {name}".to_string()]);
        let fixture = RenameFixture::live(&["fix-auth"])
            .with_config(ServiceFixture::config().with_agents(vec![another_agent]));

        fixture
            .service()
            .rename_session("fix-auth", "fix-login", false)
            .unwrap();

        assert!(
            fixture
                .multiplexer()
                .calls()
                .iter()
                .all(|call| !call.starts_with("type_if_ready"))
        );
    }

    #[test]
    fn an_agent_away_from_its_prompt_keeps_the_rename() {
        let fixture =
            RenameFixture::live(&["fix-auth"]).with_config(ServiceFixture::config_with_input());
        fixture.multiplexer().fail(
            "type_if_ready",
            MultiplexerError::AgentNotReady {
                tmux_name: "ringleader-fix-login".to_string(),
            },
        );

        let renamed = fixture
            .service()
            .rename_session("fix-auth", "fix-login", false)
            .unwrap();

        assert_eq!(fixture.registry().sessions(), vec![renamed]);
    }

    #[test]
    fn rename_moves_the_marker_with_the_branch() {
        let fixture = RenameFixture::live(&["fix-auth"]);

        let renamed = fixture
            .service()
            .rename_session("fix-auth", "fix-login", false)
            .unwrap();

        let repo = Path::new(ServiceFixture::REPO);
        let original = ServiceFixture::session("fix-auth");
        assert!(fixture.workspace().has_marker(repo, renamed.branch()));
        assert!(!fixture.workspace().has_marker(repo, original.branch()));
    }

    #[test]
    fn rename_refuses_an_unmarked_branch_with_the_marking_command() {
        let fixture = RenameFixture::unmarked("fix-auth");
        let session = ServiceFixture::session("fix-auth");
        let command =
            BranchMarker::new(session.branch()).marking_command(Path::new(ServiceFixture::REPO));

        let error = fixture
            .service()
            .rename_session("fix-auth", "fix-login", false)
            .unwrap_err();

        assert!(error.to_string().contains(&command));
        assert!(error.to_string().contains("--keep-branch"));
        assert!(fixture.workspace().state_changing_calls().is_empty());
    }

    #[test]
    fn rename_with_keep_branch_renames_a_session_on_an_unmarked_branch() {
        let fixture = RenameFixture::unmarked("fix-auth");

        let renamed = fixture
            .service()
            .rename_session("fix-auth", "fix-login", true)
            .unwrap();

        assert_eq!(renamed.name().as_str(), "fix-login");
    }

    #[test]
    fn rename_with_keep_branch_leaves_the_branch() {
        let fixture = RenameFixture::live(&["fix-auth"]);

        let renamed = fixture
            .service()
            .rename_session("fix-auth", "fix-login", true)
            .unwrap();

        assert_eq!(renamed.branch(), "fix-auth");
        assert!(
            fixture
                .workspace()
                .calls()
                .iter()
                .all(|call| !call.starts_with("rename_branch"))
        );
    }

    #[test]
    fn a_failed_tmux_rename_rolls_the_branch_back() {
        let fixture = RenameFixture::live(&["fix-auth"]);
        fixture.multiplexer().fail(
            "rename_session",
            MultiplexerError::Failed {
                subcommand: "rename-session".to_string(),
                detail: "no ringleader-done line".to_string(),
            },
        );

        fixture
            .service()
            .rename_session("fix-auth", "fix-login", false)
            .unwrap_err();

        let repo = Path::new(ServiceFixture::REPO);
        assert!(fixture.workspace().has_branch(repo, "fix-auth"));
        assert!(!fixture.workspace().has_branch(repo, "fix-login"));
    }

    #[test]
    fn a_timed_out_tmux_rename_keeps_the_renamed_branch_and_persists_nothing() {
        let fixture = RenameFixture::live(&["fix-auth"]);
        fixture.multiplexer().fail(
            "rename_session",
            MultiplexerError::TimedOut {
                subcommand: "rename-session".to_string(),
                seconds: 10,
            },
        );

        fixture
            .service()
            .rename_session("fix-auth", "fix-login", false)
            .unwrap_err();

        assert!(
            fixture
                .workspace()
                .has_branch(Path::new(ServiceFixture::REPO), "fix-login")
        );
        assert_eq!(fixture.registry().save_count(), 0);
    }

    #[test]
    fn a_stopped_session_is_renamed_without_tmux() {
        let fixture = RenameFixture::stopped("fix-auth");

        fixture
            .service()
            .rename_session("fix-auth", "fix-login", false)
            .unwrap();

        assert!(fixture.multiplexer().state_changing_calls().is_empty());
        assert_eq!(
            fixture.registry().sessions()[0].name().as_str(),
            "fix-login"
        );
    }

    #[test]
    fn a_stopped_rename_whose_registry_write_fails_rolls_the_branch_back() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = RenameFixture::stopped("fix-auth")
            .with_registry(FakeSessionRegistry::with_sessions(vec![session]).failing_saves());

        fixture
            .service()
            .rename_session("fix-auth", "fix-login", false)
            .unwrap_err();

        assert!(
            fixture
                .workspace()
                .has_branch(Path::new(ServiceFixture::REPO), "fix-auth")
        );
    }

    #[test]
    fn a_live_rename_whose_registry_write_fails_keeps_the_new_names() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = RenameFixture::live(&["fix-auth"])
            .with_registry(FakeSessionRegistry::with_sessions(vec![session]).failing_saves());

        let error = fixture
            .service()
            .rename_session("fix-auth", "fix-login", false)
            .unwrap_err();

        assert!(error.to_string().contains("records it"));
        assert_eq!(fixture.multiplexer().live_names(), ["ringleader-fix-login"]);
    }

    #[test]
    fn rename_refuses_a_name_another_session_uses() {
        let fixture = RenameFixture::live(&["fix-auth", "fix-login"]);

        let error = fixture
            .service()
            .rename_session("fix-auth", "fix-login", false)
            .unwrap_err();

        assert_eq!(error.exit_code(), 3);
    }

    #[test]
    fn rename_refuses_a_name_that_holds_other_branches() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = RenameFixture::stopped("fix-auth").with_workspace(
            ServiceFixture::workspace_for(&[&session]).with_branch(
                Path::new(ServiceFixture::REPO),
                "fix-login/draft",
                ServiceFixture::TIP,
            ),
        );

        let error = fixture
            .service()
            .rename_session("fix-auth", "fix-login", false)
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("branch fix-login clashes with existing branch fix-login/draft")
        );
        assert!(fixture.workspace().state_changing_calls().is_empty());
    }

    #[test]
    fn rename_refuses_an_unknown_name_with_not_found() {
        let fixture = RenameFixture::live(&["fix-auth"]);

        let error = fixture
            .service()
            .rename_session("nope", "fix-login", false)
            .unwrap_err();

        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn rename_refuses_an_unmanaged_session() {
        let stranger = ServiceFixture::session("mystery");
        let unmanaged = ServiceFixture::waiting(&stranger).without_metadata();
        let fixture =
            ServiceFixture::new().with_multiplexer(FakeMultiplexer::new().with_live(unmanaged));

        let error = fixture
            .service()
            .rename_session("mystery", "known", false)
            .unwrap_err();

        assert_eq!(error.exit_code(), 3);
    }

    #[test]
    fn rename_refuses_a_stopped_session_live_on_another_socket() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = RenameFixture::stopped("fix-auth").with_multiplexer(
            FakeMultiplexer::new().with_session_on_socket(
                session.socket_path(),
                session.tmux_name(),
                session.worktree_path(),
            ),
        );

        let error = fixture
            .service()
            .rename_session("fix-auth", "fix-login", false)
            .unwrap_err();

        assert!(error.to_string().contains(FakeMultiplexer::SOCKET));
        assert!(fixture.workspace().state_changing_calls().is_empty());
    }

    #[test]
    fn rename_still_runs_with_no_agents_configured() {
        let fixture = RenameFixture::stopped("fix-auth")
            .with_config(ServiceFixture::config().with_agents(vec![]));

        let renamed = fixture
            .service()
            .rename_session("fix-auth", "fix-login", false);

        assert!(renamed.is_ok());
    }
}
