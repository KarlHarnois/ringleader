use std::path::Path;
use std::sync::Arc;

use super::session_envrc::SessionEnvrc;
use super::start_outcome::StartOutcome;
use crate::application::ServiceError;
use crate::application::ServicePorts;
use crate::application::agent_hooks::AgentHooks;
use crate::application::checks::use_checks::UseChecks;
use crate::application::checks::worktree_inspection::WorktreeInspection;
use crate::application::ports::{
    AgentLaunch, HostProbe, Multiplexer, SessionRegistry, SessionSpec,
};
use crate::application::reconciliation::{ManagedRow, Reconciliation};
use crate::application::session_scratch::SessionScratch;
use crate::domain::{Agent, Config, Session};

pub struct SessionRestart {
    inspection: WorktreeInspection,
    multiplexer: Arc<dyn Multiplexer>,
    host: Arc<dyn HostProbe>,
    registry: Arc<dyn SessionRegistry>,
    scratch: SessionScratch,
    envrc: SessionEnvrc,
    hooks: Arc<AgentHooks>,
}

impl SessionRestart {
    pub fn new(ports: &ServicePorts, hooks: Arc<AgentHooks>) -> Self {
        Self {
            inspection: WorktreeInspection::new(ports),
            multiplexer: ports.tmux().multiplexer(),
            host: ports.repos().host(),
            registry: ports.state().registry(),
            scratch: SessionScratch::new(ports),
            envrc: SessionEnvrc::new(ports),
            hooks,
        }
    }

    pub fn restart(
        &self,
        row: &ManagedRow,
        reconciliation: &Reconciliation,
        config: &Config,
        config_path: &Path,
    ) -> Result<StartOutcome, ServiceError> {
        Self::refuse_running(row)?;
        let session = row.session();
        let checks = UseChecks::new(self.host.as_ref(), config, config_path);
        let agent = checks.agent_for_restart(session.agent())?;
        self.check_worktree(session)?;
        let envrc = self
            .envrc
            .choose(session.worktree_path(), session.repo_path());
        let launch = envrc.applied_to(self.scratch.prepare(Self::launch(agent, row)));
        let restarted = self.revive(row, launch, reconciliation, config)?;
        self.hooks.after_start(&restarted, agent);
        Ok(StartOutcome::new(restarted, envrc.warning()))
    }

    fn refuse_running(row: &ManagedRow) -> Result<(), ServiceError> {
        if row.state().runs_agent() {
            return Err(ServiceError::Conflict(format!(
                "session {} is running, restart only revives an exited or stopped agent",
                row.session().name()
            )));
        }
        Ok(())
    }

    fn launch(agent: &Agent, row: &ManagedRow) -> AgentLaunch {
        let argv = Self::argv(agent, row);
        AgentLaunch::for_agent(agent, argv, row.session().worktree_path())
    }

    fn argv(agent: &Agent, row: &ManagedRow) -> Vec<String> {
        let name = row.session().name();
        if row.is_live() {
            agent.restart_argv(name)
        } else {
            agent.resume_argv(name)
        }
    }

    fn revive(
        &self,
        row: &ManagedRow,
        launch: AgentLaunch,
        reconciliation: &Reconciliation,
        config: &Config,
    ) -> Result<Session, ServiceError> {
        let session = row.session();
        if row.is_live() {
            self.multiplexer
                .respawn_agent(session.tmux_name(), &launch)?;
            return Ok(session.clone());
        }
        self.recreate(session, launch, reconciliation, config)
    }

    fn check_worktree(&self, session: &Session) -> Result<(), ServiceError> {
        let worktree = session.worktree_path().display();
        if !self.inspection.exists(session) {
            return Err(ServiceError::Invalid(format!(
                "worktree {worktree} is missing, run ringleader doctor"
            )));
        }
        match self.inspection.leftover_problem(session)? {
            Some(problem) => Err(ServiceError::Invalid(format!(
                "worktree {worktree} cannot hold the agent, {problem}, run ringleader doctor"
            ))),
            None => Ok(()),
        }
    }

    fn recreate(
        &self,
        session: &Session,
        launch: AgentLaunch,
        reconciliation: &Reconciliation,
        config: &Config,
    ) -> Result<Session, ServiceError> {
        let spec = SessionSpec::new(session.tmux_name(), session.metadata().clone(), launch)
            .sized(config.session_size())
            .removing_env_names(&config.agent_env_union())
            .with_key_bindings(config.key_bindings());
        let socket = self.multiplexer.create_session(&spec)?;
        let recreated = session.on_socket(&socket);
        let entries = reconciliation.entries_with(session.name().as_str(), recreated.clone());
        if let Err(error) = self.registry.save(&entries) {
            let _ = self.multiplexer.kill_session(session.tmux_name());
            return Err(ServiceError::from(error)
                .adding_context("killed the recreated tmux session, the entry is unchanged"));
        }
        Ok(recreated)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::application::ports::MultiplexerError;
    use crate::domain::AgentPane;
    use crate::test_support::{FakeMultiplexer, FakeSessionRegistry, ServiceFixture};

    struct RestartFixture;

    impl RestartFixture {
        fn session() -> Session {
            ServiceFixture::session("fix-auth")
        }

        fn stopped() -> ServiceFixture {
            let session = Self::session();
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&session))
                .with_workspace(ServiceFixture::workspace_for(&[&session]))
                .with_host(ServiceFixture::host_for(&[&session]))
        }

        fn exited() -> ServiceFixture {
            Self::stopped().with_multiplexer(
                FakeMultiplexer::new().with_live(ServiceFixture::exited(&Self::session())),
            )
        }

        fn exited_behind_a_lost_socket() -> ServiceFixture {
            Self::stopped().with_multiplexer(
                FakeMultiplexer::new()
                    .with_live(ServiceFixture::exited(&Self::session()))
                    .with_lost_socket(),
            )
        }

        fn tmux_failure() -> MultiplexerError {
            MultiplexerError::Failed {
                subcommand: "new-session".to_string(),
                detail: "server exited unexpectedly".to_string(),
            }
        }

        fn loads_envrc(fixture: &ServiceFixture) -> bool {
            fixture
                .multiplexer()
                .launches()
                .pop()
                .unwrap()
                .loads_envrc()
        }

        fn resuming_config() -> Config {
            let agent = ServiceFixture::agent().with_resume_args(&["--continue".to_string()]);
            ServiceFixture::config().with_agents(vec![agent])
        }

        fn recreated_argv(fixture: &ServiceFixture) -> Vec<String> {
            let spec = fixture.multiplexer().specs().pop().unwrap();
            spec.launch().argv().to_vec()
        }

        fn restart(fixture: &ServiceFixture) -> Result<(), ServiceError> {
            fixture.service().restart_session("fix-auth").map(drop)
        }
    }

    #[test]
    fn an_exited_session_is_respawned_without_its_prompt() {
        let fixture = RestartFixture::exited();

        RestartFixture::restart(&fixture).unwrap();

        let launch = fixture.multiplexer().launches().pop().unwrap();
        assert_eq!(launch.argv(), ["claude"]);
        assert!(
            fixture
                .multiplexer()
                .calls()
                .iter()
                .any(|call| call.starts_with("respawn_agent"))
        );
    }

    #[test]
    fn an_exited_session_behind_a_lost_socket_is_respawned_once_the_socket_is_back() {
        let fixture = RestartFixture::exited_behind_a_lost_socket();

        RestartFixture::restart(&fixture).unwrap();

        let calls = fixture.multiplexer().state_changing_calls();
        assert!(calls.iter().any(|call| call.starts_with("respawn_agent")));
        assert!(calls.iter().all(|call| !call.starts_with("create_session")));
    }

    #[test]
    fn a_restart_that_recreates_the_socket_says_so() {
        let fixture = RestartFixture::exited_behind_a_lost_socket();

        let outcome = fixture.service().restart_session("fix-auth").unwrap();

        assert!(outcome.recovery().unwrap().contains("recreated the socket"));
    }

    #[test]
    fn a_restart_applies_a_drifted_server_config_before_it_recreates_the_session() {
        let fixture = RestartFixture::stopped()
            .with_multiplexer(FakeMultiplexer::new().with_drifted_config("M-x"));

        RestartFixture::restart(&fixture).unwrap();

        let calls = fixture.multiplexer().calls();
        let applied = calls
            .iter()
            .position(|call| call == "apply_config C-q M-j M-Down M-k M-Up M-l");
        let created = calls
            .iter()
            .position(|call| call.starts_with("create_session"));
        assert!(
            applied.is_some_and(|applied| Some(applied) < created),
            "{calls:?}"
        );
    }

    #[test]
    fn restart_keeps_the_agent_args() {
        let agent = ServiceFixture::agent().with_args(&["--model".to_string(), "opus".to_string()]);
        let fixture =
            RestartFixture::exited().with_config(ServiceFixture::config().with_agents(vec![agent]));

        RestartFixture::restart(&fixture).unwrap();

        let launch = fixture.multiplexer().launches().pop().unwrap();
        assert_eq!(launch.argv(), ["claude", "--model", "opus"]);
    }

    #[test]
    fn restart_passes_the_session_name_to_the_agent() {
        let fixture = RestartFixture::exited().with_config(ServiceFixture::config_with_input());

        RestartFixture::restart(&fixture).unwrap();

        let launch = fixture.multiplexer().launches().pop().unwrap();
        assert_eq!(launch.argv(), ["claude", "--name", "fix-auth"]);
    }

    #[test]
    fn a_respawned_agent_gets_its_startup_input() {
        let fixture = RestartFixture::exited().with_config(ServiceFixture::config_with_input());

        RestartFixture::restart(&fixture).unwrap();

        assert!(
            fixture
                .multiplexer()
                .wait_for_typing("ringleader-fix-auth", Duration::from_secs(2))
        );
        assert_eq!(
            fixture
                .multiplexer()
                .typed_when_ready("ringleader-fix-auth"),
            ["/color"]
        );
    }

    #[test]
    fn a_recreated_session_gets_its_startup_input() {
        let fixture = RestartFixture::stopped().with_config(ServiceFixture::config_with_input());

        RestartFixture::restart(&fixture).unwrap();

        assert!(
            fixture
                .multiplexer()
                .wait_for_typing("ringleader-fix-auth", Duration::from_secs(2))
        );
        assert_eq!(
            fixture
                .multiplexer()
                .typed_when_ready("ringleader-fix-auth"),
            ["/color"]
        );
    }

    #[test]
    fn restart_returns_before_the_startup_input_is_typed() {
        let session = RestartFixture::session();
        let fixture = RestartFixture::exited()
            .with_config(ServiceFixture::config_with_input())
            .with_multiplexer(
                FakeMultiplexer::new()
                    .with_live(ServiceFixture::exited(&session))
                    .blocking_typing(),
            );
        let mut service = fixture.service();
        let started = Instant::now();

        service.restart_session("fix-auth").unwrap();

        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(
            fixture
                .multiplexer()
                .typed_when_ready("ringleader-fix-auth")
                .is_empty()
        );
        fixture.multiplexer().release_typing();
        service.wait_startup_input(|| false);
        assert_eq!(
            fixture
                .multiplexer()
                .typed_when_ready("ringleader-fix-auth"),
            ["/color"]
        );
    }

    #[test]
    fn restart_keeps_the_agent_set_env() {
        let assignments = [(
            "EXAMPLE_ACCOUNT_DIR".to_string(),
            "/accounts/example".to_string(),
        )];
        let agent = ServiceFixture::agent().with_env_assignments(&assignments);
        let fixture =
            RestartFixture::exited().with_config(ServiceFixture::config().with_agents(vec![agent]));

        RestartFixture::restart(&fixture).unwrap();

        let launch = fixture.multiplexer().launches().pop().unwrap();
        assert!(launch.env_assignments().starts_with(&assignments));
    }

    #[test]
    fn restart_points_the_agent_tmpdir_at_the_session_scratch_directory() {
        let fixture = RestartFixture::exited();

        RestartFixture::restart(&fixture).unwrap();

        let launch = fixture.multiplexer().launches().pop().unwrap();
        assert_eq!(
            launch.env_assignments(),
            [fixture.tmpdir_assignment("fix-auth")]
        );
    }

    #[test]
    fn an_exited_session_without_a_tagged_pane_gets_a_new_agent_window() {
        let session = RestartFixture::session();
        let without_pane = crate::domain::SessionObservation::new(
            crate::domain::TmuxAddress::new(session.tmux_name(), session.socket_path()),
            ServiceFixture::CREATED_AT,
        )
        .with_metadata(session.metadata().clone());
        let fixture = RestartFixture::stopped()
            .with_multiplexer(FakeMultiplexer::new().with_live(without_pane));

        RestartFixture::restart(&fixture).unwrap();

        assert!(
            fixture
                .multiplexer()
                .calls()
                .iter()
                .any(|call| call.starts_with("respawn_agent"))
        );
    }

    #[test]
    fn a_stopped_session_is_recreated_in_its_worktree_and_recorded() {
        let fixture = RestartFixture::stopped();

        RestartFixture::restart(&fixture).unwrap();

        let spec = fixture.multiplexer().specs().pop().unwrap();
        assert_eq!(spec.tmux_name(), "ringleader-fix-auth");
        assert_eq!(spec.worktree(), RestartFixture::session().worktree_path());
        assert_eq!(
            fixture.registry().sessions()[0].created_at(),
            ServiceFixture::CREATED_AT
        );
        assert!(fixture.workspace().state_changing_calls().is_empty());
    }

    #[test]
    fn a_stopped_session_in_the_main_checkout_is_recreated_in_the_checkout() {
        let session = ServiceFixture::main_checkout_session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_workspace(ServiceFixture::main_checkout_workspace())
            .with_host(ServiceFixture::host_for(&[&session]));

        RestartFixture::restart(&fixture).unwrap();

        let spec = fixture.multiplexer().specs().pop().unwrap();
        assert_eq!(spec.worktree(), Path::new(ServiceFixture::REPO));
    }

    #[test]
    fn a_stopped_session_is_recreated_with_the_agent_resume_args() {
        let fixture = RestartFixture::stopped().with_config(RestartFixture::resuming_config());

        RestartFixture::restart(&fixture).unwrap();

        assert_eq!(
            RestartFixture::recreated_argv(&fixture),
            ["claude", "--continue"]
        );
    }

    #[test]
    fn an_exited_session_skips_resume_args_because_a_resume_with_no_conversation_exits() {
        let fixture = RestartFixture::exited().with_config(RestartFixture::resuming_config());

        RestartFixture::restart(&fixture).unwrap();

        let launch = fixture.multiplexer().launches().pop().unwrap();
        assert_eq!(launch.argv(), ["claude"]);
    }

    #[test]
    fn a_stopped_session_recreated_through_its_envrc_keeps_the_resume_args() {
        let fixture = RestartFixture::stopped()
            .with_config(RestartFixture::resuming_config())
            .with_directory_environment(ServiceFixture::allowed_envrc("fix-auth"));

        RestartFixture::restart(&fixture).unwrap();

        assert_eq!(
            RestartFixture::recreated_argv(&fixture),
            ["claude", "--continue"]
        );
    }

    #[test]
    fn a_recreated_session_carries_the_configured_key_bindings() {
        let fixture = RestartFixture::stopped().with_config(ServiceFixture::config_with_alt_n());

        RestartFixture::restart(&fixture).unwrap();

        let spec = fixture.multiplexer().specs().pop().unwrap();
        assert_eq!(spec.key_bindings(), &ServiceFixture::alt_n_bindings());
    }

    #[test]
    fn a_running_session_refuses_restart() {
        let session = RestartFixture::session();
        let fixture = RestartFixture::stopped().with_multiplexer(FakeMultiplexer::new().with_live(
            ServiceFixture::observed(&session, AgentPane::live(ServiceFixture::NOW)),
        ));

        let error = RestartFixture::restart(&fixture).unwrap_err();

        assert!(matches!(error, ServiceError::Conflict(_)));
    }

    #[test]
    fn a_waiting_session_refuses_restart() {
        let session = RestartFixture::session();
        let fixture = RestartFixture::stopped()
            .with_multiplexer(FakeMultiplexer::new().with_live(ServiceFixture::waiting(&session)));

        let error = RestartFixture::restart(&fixture).unwrap_err();

        assert!(matches!(error, ServiceError::Conflict(_)));
    }

    #[test]
    fn an_unmanaged_session_refuses_restart() {
        let stranger = ServiceFixture::session("mystery");
        let fixture = ServiceFixture::new().with_multiplexer(
            FakeMultiplexer::new().with_live(ServiceFixture::waiting(&stranger).without_metadata()),
        );

        let error = fixture.service().restart_session("mystery").unwrap_err();

        assert!(matches!(error, ServiceError::Conflict(_)));
    }

    #[test]
    fn a_shadowed_entry_refuses_restart_with_the_kill_advice() {
        let session = RestartFixture::session();
        let fixture = RestartFixture::stopped().with_multiplexer(
            FakeMultiplexer::new().with_live(ServiceFixture::waiting(&session).without_metadata()),
        );

        let error = RestartFixture::restart(&fixture).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("kill-session -t '=ringleader-fix-auth:'")
        );
    }

    #[test]
    fn a_failed_tmux_step_changes_no_workspace_state_and_keeps_the_entry() {
        let fixture = RestartFixture::stopped().with_multiplexer(
            FakeMultiplexer::new().failing("create_session", RestartFixture::tmux_failure()),
        );

        RestartFixture::restart(&fixture).unwrap_err();

        assert!(fixture.workspace().state_changing_calls().is_empty());
        assert_eq!(fixture.registry().save_count(), 0);
    }

    #[test]
    fn a_failed_respawn_changes_no_workspace_state() {
        let fixture = RestartFixture::exited();
        fixture
            .multiplexer()
            .fail("respawn_agent", RestartFixture::tmux_failure());

        RestartFixture::restart(&fixture).unwrap_err();

        assert!(fixture.workspace().state_changing_calls().is_empty());
        assert_eq!(fixture.registry().sessions(), [RestartFixture::session()]);
    }

    #[test]
    fn a_failed_registry_write_kills_the_recreated_session() {
        let fixture = RestartFixture::stopped().with_registry(
            FakeSessionRegistry::with_sessions(vec![RestartFixture::session()]).failing_saves(),
        );

        RestartFixture::restart(&fixture).unwrap_err();

        assert!(fixture.multiplexer().live_names().is_empty());
    }

    #[test]
    fn a_missing_worktree_fails_before_tmux() {
        let fixture = RestartFixture::stopped().with_host(ServiceFixture::host_for(&[]));

        let error = RestartFixture::restart(&fixture).unwrap_err();

        assert!(error.to_string().contains("doctor"));
        assert!(fixture.multiplexer().state_changing_calls().is_empty());
    }

    #[test]
    fn a_worktree_git_does_not_list_fails_before_tmux() {
        let fixture = RestartFixture::exited().with_workspace(ServiceFixture::workspace_for(&[]));

        RestartFixture::restart(&fixture).unwrap_err();

        assert!(fixture.multiplexer().state_changing_calls().is_empty());
    }

    #[test]
    fn a_worktree_without_its_git_file_fails_before_tmux() {
        let session = RestartFixture::session();
        let fixture = RestartFixture::stopped()
            .with_host(ServiceFixture::host_for(&[]).with_directory(session.worktree_path()));

        RestartFixture::restart(&fixture).unwrap_err();

        assert!(fixture.multiplexer().state_changing_calls().is_empty());
    }

    #[test]
    fn a_removed_agent_refuses_restart_before_tmux() {
        let fixture =
            RestartFixture::exited().with_config(ServiceFixture::config().with_agents(vec![]));

        let error = RestartFixture::restart(&fixture).unwrap_err();

        assert!(matches!(error, ServiceError::Invalid(_)));
        assert!(fixture.multiplexer().state_changing_calls().is_empty());
    }

    #[test]
    fn an_unresolvable_command_refuses_restart_before_tmux() {
        let session = RestartFixture::session();
        let fixture = RestartFixture::stopped().with_host(
            crate::test_support::FakeHostProbe::default()
                .with_directory(session.worktree_path())
                .with_file(&session.worktree_path().join(".git")),
        );

        let error = RestartFixture::restart(&fixture).unwrap_err();

        assert!(error.to_string().contains("was not found"));
        assert!(fixture.multiplexer().state_changing_calls().is_empty());
    }

    #[test]
    fn a_stopped_session_refuses_restart_while_its_agent_runs_under_the_private_server() {
        let fixture = RestartFixture::stopped().with_host(ServiceFixture::with_worktree_agent(
            &RestartFixture::session(),
        ));

        let error = RestartFixture::restart(&fixture).unwrap_err();

        assert!(matches!(error, ServiceError::Conflict(_)));
        assert!(
            error
                .to_string()
                .contains("Ringleader's tmux server still runs processes in its worktree")
        );
        assert!(fixture.multiplexer().state_changing_calls().is_empty());
    }

    #[test]
    fn a_stopped_session_refuses_restart_while_its_agent_runs_on_its_recorded_server() {
        let recorded = Path::new("/home/example/.local/state/ringleader/tmux/tmux.sock");
        let session = RestartFixture::session().on_socket(recorded);
        let host = ServiceFixture::with_worktree_process(&session)
            .serving(ServiceFixture::WORKTREE_PROCESS, recorded);
        let fixture = RestartFixture::stopped()
            .with_sessions(std::slice::from_ref(&session))
            .with_host(host);

        let error = RestartFixture::restart(&fixture).unwrap_err();

        assert!(
            error
                .to_string()
                .contains(&format!("tmux -S {} kill-session", recorded.display()))
        );
        assert!(fixture.multiplexer().state_changing_calls().is_empty());
    }

    #[test]
    fn a_stopped_session_whose_recorded_server_still_runs_is_recreated_here() {
        let recorded = Path::new(ServiceFixture::OTHER_RUNTIME_SOCKET);
        let session = RestartFixture::session().on_socket(recorded);
        let fixture = RestartFixture::stopped()
            .with_sessions(std::slice::from_ref(&session))
            .with_host(ServiceFixture::host_for(&[&session]).with_tmux_server_on(recorded));

        RestartFixture::restart(&fixture).unwrap();

        assert_eq!(
            fixture.registry().sessions()[0].socket_path(),
            Path::new(FakeMultiplexer::SOCKET)
        );
    }

    #[test]
    fn an_editor_open_in_a_stopped_worktree_does_not_block_restart() {
        let fixture = RestartFixture::stopped().with_host(ServiceFixture::with_worktree_process(
            &RestartFixture::session(),
        ));

        RestartFixture::restart(&fixture).unwrap();

        assert_eq!(fixture.multiplexer().launches().len(), 1);
    }

    #[test]
    fn a_stopped_entry_whose_worktree_a_renamed_out_session_claims_refuses_restart() {
        let session = RestartFixture::session();
        let claimant = crate::domain::SessionObservation::new(
            crate::domain::TmuxAddress::new("my-own-name", session.socket_path()),
            ServiceFixture::CREATED_AT,
        )
        .with_agent_pane(AgentPane::live(0))
        .with_metadata(session.metadata().clone());
        let fixture =
            RestartFixture::stopped().with_multiplexer(FakeMultiplexer::new().with_live(claimant));

        let error = RestartFixture::restart(&fixture).unwrap_err();

        assert!(error.to_string().contains("claims the worktree"));
    }

    #[test]
    fn an_unknown_name_is_not_found() {
        let fixture = RestartFixture::stopped();

        let error = fixture.service().restart_session("nope").unwrap_err();

        assert!(matches!(error, ServiceError::NotFound(_)));
    }

    #[test]
    fn a_timed_out_recreation_keeps_the_entry_unchanged() {
        let fixture = RestartFixture::stopped().with_multiplexer(FakeMultiplexer::new().failing(
            "create_session",
            MultiplexerError::TimedOut {
                subcommand: "new-session".to_string(),
                seconds: 10,
            },
        ));

        RestartFixture::restart(&fixture).unwrap_err();

        assert_eq!(fixture.registry().save_count(), 0);
    }

    #[test]
    fn an_entry_under_an_older_naming_scheme_is_still_restartable() {
        let current = RestartFixture::session();
        let old_naming = crate::domain::NamingScheme::new(
            std::path::Path::new(ServiceFixture::WORKTREE_ROOT),
            "old-",
        );
        let session = Session::new(
            current.name().clone(),
            current.metadata().with_branch("old/fix-auth"),
            crate::domain::Registration::new(
                crate::domain::TmuxAddress::new("old-fix-auth", current.socket_path()),
                old_naming,
                ServiceFixture::CREATED_AT,
            ),
        );
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_workspace(ServiceFixture::workspace_for(&[&session]))
            .with_host(ServiceFixture::host_for(&[&session]))
            .with_multiplexer(FakeMultiplexer::new().with_live(ServiceFixture::exited(&session)));

        assert!(RestartFixture::restart(&fixture).is_ok());
    }

    #[test]
    fn an_exited_session_is_respawned_through_its_allowed_envrc() {
        let fixture = RestartFixture::exited()
            .with_directory_environment(ServiceFixture::allowed_envrc("fix-auth"));

        RestartFixture::restart(&fixture).unwrap();

        assert!(RestartFixture::loads_envrc(&fixture));
    }

    #[test]
    fn a_stopped_session_is_recreated_through_its_allowed_envrc() {
        let fixture = RestartFixture::stopped()
            .with_directory_environment(ServiceFixture::allowed_envrc("fix-auth"));

        RestartFixture::restart(&fixture).unwrap();

        assert!(
            fixture
                .multiplexer()
                .specs()
                .pop()
                .unwrap()
                .launch()
                .loads_envrc()
        );
    }

    #[test]
    fn a_restart_through_the_envrc_passes_the_agent_its_args_and_the_session_name() {
        let fixture = RestartFixture::exited()
            .with_config(ServiceFixture::config_with_input())
            .with_directory_environment(ServiceFixture::allowed_envrc("fix-auth"));

        RestartFixture::restart(&fixture).unwrap();

        let launch = fixture.multiplexer().launches().pop().unwrap();
        assert_eq!(launch.argv(), ["claude", "--name", "fix-auth"]);
        assert!(launch.loads_envrc());
    }

    #[test]
    fn a_restart_allows_an_envrc_matching_the_main_checkout() {
        let environment = ServiceFixture::not_allowed_envrc("fix-auth")
            .with_allowed_envrc(Path::new(ServiceFixture::REPO), ServiceFixture::ENVRC);
        let fixture = RestartFixture::stopped().with_directory_environment(environment);

        RestartFixture::restart(&fixture).unwrap();

        assert_eq!(
            fixture.directory_environment().allowed(),
            [RestartFixture::session().worktree_path()]
        );
    }

    #[test]
    fn a_not_allowed_envrc_still_restarts_the_agent_and_returns_the_warning() {
        let fixture = RestartFixture::exited()
            .with_directory_environment(ServiceFixture::not_allowed_envrc("fix-auth"));

        let outcome = fixture.service().restart_session("fix-auth").unwrap();

        assert!(outcome.warning().unwrap().contains("is not allowed"));
        assert!(!RestartFixture::loads_envrc(&fixture));
    }
}
