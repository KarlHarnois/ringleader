use std::sync::Arc;

use super::agent_hooks::AgentHooks;
use super::branch_availability::BranchAvailability;
use super::branch_ownership::{BranchMutation, BranchOwnership};
use super::branch_renaming::BranchRenaming;
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
        let renamed = row.session().renamed(SessionName::new(request.new_name())?);
        Self::refuse_taken_name(&renamed, reconciliation)?;
        let renaming = BranchRenaming::new(
            self.workspace.as_ref(),
            self.host.as_ref(),
            row.session(),
            &renamed,
        );
        if renaming.changes_branch() {
            self.refuse_branch_change(row.session(), &renamed)?;
        }
        renaming.apply()?;
        if row.is_live() {
            self.rename_tmux_session(row, &renamed, &renaming)?;
        }
        let entries = reconciliation.entries_with(row.session().name().as_str(), renamed.clone());
        self.persist_rename(row, &entries, &renaming)?;
        renaming.drop_stale_upstream();
        self.tell_agent_its_name(row, &renamed, config);
        Ok(renamed)
    }

    fn refuse_taken_name(
        renamed: &Session,
        reconciliation: &Reconciliation,
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
        Ok(())
    }

    fn refuse_branch_change(
        &self,
        session: &Session,
        renamed: &Session,
    ) -> Result<(), ServiceError> {
        self.branches
            .refuse_taken(session.repo_path(), renamed.branch())?;
        self.refuse_published(session)?;
        self.ownership
            .refuse_unmarked(session, BranchMutation::Rename)
    }

    fn refuse_published(&self, session: &Session) -> Result<(), ServiceError> {
        let branch = session.branch();
        let repo = session.repo_path();
        if self.workspace.is_branch_published(repo, branch)? {
            return Err(ServiceError::Conflict(format!(
                "branch {branch} has been pushed, refusing to rename it, since the remote branch and any pull request keep the name {branch}; if that remote branch was deleted, run git fetch --all --prune in {} and retry",
                repo.display()
            )));
        }
        Ok(())
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

        fn live_with_failing_tmux_rename(name: &str) -> ServiceFixture {
            let fixture = Self::live(&[name]);
            fixture.multiplexer().fail(
                "rename_session",
                MultiplexerError::Failed {
                    subcommand: "rename-session".to_string(),
                    detail: "no ringleader-done line".to_string(),
                },
            );
            fixture
        }

        fn unmarked(name: &str) -> ServiceFixture {
            let session = ServiceFixture::session(name);
            let repo = Path::new(ServiceFixture::REPO);
            Self::stopped(name).with_workspace(
                ServiceFixture::workspace_for(&[&session]).without_marker(repo, session.branch()),
            )
        }

        fn published(name: &str) -> ServiceFixture {
            let session = ServiceFixture::session(name);
            let repo = Path::new(ServiceFixture::REPO);
            Self::stopped(name).with_workspace(
                ServiceFixture::workspace_for(&[&session])
                    .with_published_branch(repo, session.branch()),
            )
        }

        fn on_branch(name: &str, branch: &str) -> ServiceFixture {
            Self::registered(ServiceFixture::session_on_branch(name, branch))
        }

        fn stopped(name: &str) -> ServiceFixture {
            Self::registered(ServiceFixture::session(name))
        }

        fn registered(session: Session) -> ServiceFixture {
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
            .rename_session("fix-auth", "fix-login")
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
            .rename_session("fix-auth", "fix-login")
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
            .rename_session("fix-auth", "fix-login")
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
            .rename_session("fix-auth", "fix-login")
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
            .rename_session("fix-auth", "fix-login")
            .unwrap();

        assert_eq!(fixture.registry().sessions(), vec![renamed]);
    }

    #[test]
    fn rename_moves_the_marker_with_the_branch() {
        let fixture = RenameFixture::live(&["fix-auth"]);

        let renamed = fixture
            .service()
            .rename_session("fix-auth", "fix-login")
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
            .rename_session("fix-auth", "fix-login")
            .unwrap_err();

        assert!(error.to_string().contains(&command));
        assert!(fixture.workspace().state_changing_calls().is_empty());
    }

    #[test]
    fn an_unmarked_branch_refusal_says_the_session_keeps_its_name() {
        let fixture = RenameFixture::unmarked("fix-auth");

        let error = fixture
            .service()
            .rename_session("fix-auth", "fix-login")
            .unwrap_err();

        assert!(error.to_string().ends_with(
            "otherwise the session keeps its name, since rename moves the branch with it"
        ));
    }

    #[test]
    fn a_pushed_branch_without_the_marker_is_refused_as_pushed() {
        let session = ServiceFixture::session("fix-auth");
        let repo = Path::new(ServiceFixture::REPO);
        let fixture = RenameFixture::stopped("fix-auth").with_workspace(
            ServiceFixture::workspace_for(&[&session])
                .without_marker(repo, session.branch())
                .with_published_branch(repo, session.branch()),
        );

        let error = fixture
            .service()
            .rename_session("fix-auth", "fix-login")
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("branch fix-auth has been pushed")
        );
    }

    #[test]
    fn rename_refuses_a_pushed_branch() {
        let fixture = RenameFixture::published("fix-auth");

        let error = fixture
            .service()
            .rename_session("fix-auth", "fix-login")
            .unwrap_err();

        assert_eq!(error.exit_code(), 3);
        assert!(
            error
                .to_string()
                .contains("branch fix-auth has been pushed")
        );
        assert!(error.to_string().contains("git fetch --all --prune"));
        assert!(fixture.workspace().state_changing_calls().is_empty());
    }

    #[test]
    fn rename_moves_a_legacy_branch_to_the_new_name() {
        let fixture = RenameFixture::on_branch("fix-auth", "ringleader/fix-auth");

        let renamed = fixture
            .service()
            .rename_session("fix-auth", "fix-login")
            .unwrap();

        assert_eq!(renamed.branch(), "fix-login");
        assert!(
            fixture
                .workspace()
                .has_branch(Path::new(ServiceFixture::REPO), "fix-login")
        );
    }

    #[test]
    fn rename_onto_the_current_branch_skips_the_branch_checks() {
        let fixture = RenameFixture::on_branch("fix-auth", "fix-login");

        let renamed = fixture
            .service()
            .rename_session("fix-auth", "fix-login")
            .unwrap();

        assert_eq!(renamed.branch(), "fix-login");
        assert!(fixture.workspace().state_changing_calls().is_empty());
    }

    #[test]
    fn rename_onto_a_pushed_unmarked_current_branch_succeeds() {
        let session = ServiceFixture::session_on_branch("auth", "fix-auth");
        let repo = Path::new(ServiceFixture::REPO);
        let fixture = RenameFixture::on_branch("auth", "fix-auth").with_workspace(
            ServiceFixture::workspace_for(&[&session])
                .with_published_branch(repo, "fix-auth")
                .without_marker(repo, "fix-auth"),
        );

        let renamed = fixture
            .service()
            .rename_session("auth", "fix-auth")
            .unwrap();

        assert_eq!(renamed.name().as_str(), "fix-auth");
    }

    #[test]
    fn a_failed_tmux_rename_rolls_the_branch_back() {
        let fixture = RenameFixture::live_with_failing_tmux_rename("fix-auth");

        fixture
            .service()
            .rename_session("fix-auth", "fix-login")
            .unwrap_err();

        let repo = Path::new(ServiceFixture::REPO);
        assert!(fixture.workspace().has_branch(repo, "fix-auth"));
        assert!(!fixture.workspace().has_branch(repo, "fix-login"));
    }

    #[test]
    fn a_rolled_back_rename_leaves_the_upstream_alone() {
        let fixture = RenameFixture::live_with_failing_tmux_rename("fix-auth");

        fixture
            .service()
            .rename_session("fix-auth", "fix-login")
            .unwrap_err();

        assert!(
            fixture
                .workspace()
                .calls()
                .iter()
                .all(|call| !call.starts_with("drop_stale_upstream"))
        );
    }

    #[test]
    fn a_recorded_rename_drops_the_upstream_left_on_the_old_name() {
        let fixture = RenameFixture::stopped("fix-auth");

        fixture
            .service()
            .rename_session("fix-auth", "fix-login")
            .unwrap();

        let calls = fixture.workspace().state_changing_calls();
        let last = calls.last().unwrap();
        assert!(last.starts_with("drop_stale_upstream"));
        assert!(last.ends_with("fix-auth fix-login"));
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
            .rename_session("fix-auth", "fix-login")
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
            .rename_session("fix-auth", "fix-login")
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
            .rename_session("fix-auth", "fix-login")
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
            .rename_session("fix-auth", "fix-login")
            .unwrap_err();

        assert!(error.to_string().contains("records it"));
        assert_eq!(fixture.multiplexer().live_names(), ["ringleader-fix-login"]);
    }

    #[test]
    fn rename_refuses_a_name_another_session_uses() {
        let fixture = RenameFixture::live(&["fix-auth", "fix-login"]);

        let error = fixture
            .service()
            .rename_session("fix-auth", "fix-login")
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
            .rename_session("fix-auth", "fix-login")
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
            .rename_session("nope", "fix-login")
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
            .rename_session("mystery", "known")
            .unwrap_err();

        assert_eq!(error.exit_code(), 3);
    }

    #[test]
    fn rename_refuses_a_stopped_session_while_its_agent_runs_under_the_private_server() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = RenameFixture::stopped("fix-auth")
            .with_host(ServiceFixture::with_worktree_agent(&session));

        let error = fixture
            .service()
            .rename_session("fix-auth", "fix-login")
            .unwrap_err();

        assert_eq!(error.exit_code(), 3);
        assert!(
            error
                .to_string()
                .contains("Ringleader's tmux server still runs processes in its worktree")
        );
        assert!(fixture.workspace().state_changing_calls().is_empty());
    }

    #[test]
    fn rename_still_runs_with_no_agents_configured() {
        let fixture = RenameFixture::stopped("fix-auth")
            .with_config(ServiceFixture::config().with_agents(vec![]));

        let renamed = fixture.service().rename_session("fix-auth", "fix-login");

        assert!(renamed.is_ok());
    }
}
