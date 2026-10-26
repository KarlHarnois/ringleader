use super::ServiceError;
use super::ServicePorts;
use super::ports::ProcessInfo;
use super::reconciliation::ManagedRow;
use super::stop_skip::StopSkip;
use super::tmux_teardown::{TeardownContexts, TmuxTeardown};

pub struct SessionStop {
    teardown: TmuxTeardown,
}

impl SessionStop {
    const KEPT_ON_TMUX_FAILURE: &'static str = "kept the session, run ringleader doctor";
    const KEPT_WHILE_RUNNING: &'static str = "kept the session, which still runs";
    const SESSION_STOPPED: &'static str =
        "the session is now stopped and a second stop cannot find them";
    const TEARDOWN_CONTEXTS: TeardownContexts = TeardownContexts::new(
        Self::KEPT_ON_TMUX_FAILURE,
        Self::KEPT_WHILE_RUNNING,
        Self::SESSION_STOPPED,
    );

    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            teardown: TmuxTeardown::new(ports, Self::TEARDOWN_CONTEXTS),
        }
    }

    pub fn stop(&self, row: &ManagedRow) -> Result<(), ServiceError> {
        Self::refuse_ineligible(row)?;
        let survivors = self.teardown.stop(row, &[])?;
        Self::refuse_survivors(&survivors)
    }

    fn refuse_ineligible(row: &ManagedRow) -> Result<(), ServiceError> {
        let name = row.session().name();
        match StopSkip::of(row.session(), row.state()) {
            None => Ok(()),
            Some(StopSkip::State(state)) => Err(ServiceError::Conflict(format!(
                "session {name} is {}, stop only ends a waiting or exited agent",
                state.label()
            ))),
            Some(StopSkip::MainCheckout) => Err(ServiceError::Conflict(format!(
                "session {name} runs in the main checkout, where a restart cannot resume its conversation, so it cannot be stopped"
            ))),
        }
    }

    fn refuse_survivors(survivors: &[ProcessInfo]) -> Result<(), ServiceError> {
        if survivors.is_empty() {
            return Ok(());
        }
        Err(ServiceError::Conflict(format!(
            "processes the agent started still run: {}; end them by hand, since {}",
            ProcessInfo::listed(survivors),
            Self::SESSION_STOPPED
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AgentPane, Session, SessionObservation, SessionState};
    use crate::test_support::{FakeHostProbe, FakeMultiplexer, ServiceFixture};

    struct StopFixture;

    impl StopFixture {
        const PANE_SESSION: u32 = 4100;
        const DEV_SERVER: u32 = 4200;

        fn session() -> Session {
            ServiceFixture::session("fix-auth")
        }

        fn stopped() -> ServiceFixture {
            let session = Self::session();
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&session))
                .with_workspace(ServiceFixture::workspace_for(&[&session]))
                .with_host(Self::dev_server_host())
        }

        fn waiting() -> ServiceFixture {
            Self::live(ServiceFixture::waiting(&Self::session()))
        }

        fn live(observation: SessionObservation) -> ServiceFixture {
            let multiplexer = FakeMultiplexer::new()
                .with_live(observation)
                .with_pane_pids(Self::session().tmux_name(), &[Self::PANE_SESSION]);
            Self::stopped().with_multiplexer(multiplexer)
        }

        fn dev_server_host() -> FakeHostProbe {
            let session = Self::session();
            ServiceFixture::host_for(&[&session]).with_process(
                Self::DEV_SERVER,
                "npm run dev",
                session.worktree_path(),
                Self::PANE_SESSION,
            )
        }

        fn main_checkout_waiting() -> ServiceFixture {
            let session = ServiceFixture::main_checkout_session("fix-auth");
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&session))
                .with_workspace(ServiceFixture::main_checkout_workspace())
                .with_multiplexer(
                    FakeMultiplexer::new().with_live(ServiceFixture::waiting(&session)),
                )
        }

        fn resuming(fixture: ServiceFixture) -> ServiceFixture {
            let agent = ServiceFixture::agent().with_resume_args(&["--continue".to_string()]);
            fixture.with_config(ServiceFixture::config().with_agents(vec![agent]))
        }

        fn stop(fixture: &ServiceFixture) -> Result<(), ServiceError> {
            fixture.service().stop_session("fix-auth")
        }

        fn state(fixture: &ServiceFixture) -> SessionState {
            let mut service = fixture.service();
            let rows = service.observe().unwrap();
            rows.iter()
                .find(|row| row.name() == "fix-auth")
                .unwrap()
                .state()
        }
    }

    #[test]
    fn stopping_a_waiting_session_ends_its_tmux_session() {
        let fixture = StopFixture::waiting();

        StopFixture::stop(&fixture).unwrap();

        assert!(fixture.multiplexer().live_names().is_empty());
    }

    #[test]
    fn a_stopped_session_keeps_its_entry_so_it_lists_as_stopped() {
        let fixture = StopFixture::waiting();

        StopFixture::stop(&fixture).unwrap();

        assert_eq!(fixture.registry().sessions(), [StopFixture::session()]);
        assert_eq!(StopFixture::state(&fixture), SessionState::Stopped);
    }

    #[test]
    fn stopping_ends_the_processes_the_agent_started_to_free_their_memory() {
        let fixture = StopFixture::waiting();

        StopFixture::stop(&fixture).unwrap();

        assert_eq!(fixture.host().terminated(), [StopFixture::DEV_SERVER]);
    }

    #[test]
    fn a_stopped_session_restarts_with_its_resume_args_so_its_conversation_comes_back() {
        let fixture = StopFixture::resuming(StopFixture::waiting());
        StopFixture::stop(&fixture).unwrap();

        fixture.service().restart_session("fix-auth").unwrap();

        let spec = fixture.multiplexer().specs().pop().unwrap();
        assert_eq!(spec.launch().argv(), ["claude", "--continue"]);
    }

    #[test]
    fn an_exited_session_can_be_stopped() {
        let fixture = StopFixture::live(ServiceFixture::exited(&StopFixture::session()));

        StopFixture::stop(&fixture).unwrap();

        assert!(fixture.multiplexer().live_names().is_empty());
    }

    #[test]
    fn a_working_session_refuses_stop_because_its_turn_would_be_lost() {
        let fixture = StopFixture::live(ServiceFixture::observed(
            &StopFixture::session(),
            AgentPane::live(ServiceFixture::NOW),
        ));

        let error = StopFixture::stop(&fixture).unwrap_err();

        assert!(error.to_string().contains("is working"), "{error}");
        assert_eq!(fixture.multiplexer().live_names().len(), 1);
    }

    #[test]
    fn a_stopped_session_refuses_stop() {
        let fixture = StopFixture::stopped();

        let error = StopFixture::stop(&fixture).unwrap_err();

        assert!(matches!(error, ServiceError::Conflict(_)));
    }

    #[test]
    fn a_main_checkout_session_refuses_stop_because_its_restart_starts_fresh() {
        let fixture = StopFixture::main_checkout_waiting();

        let error = StopFixture::stop(&fixture).unwrap_err();

        assert!(error.to_string().contains("main checkout"), "{error}");
        assert_eq!(fixture.multiplexer().live_names().len(), 1);
    }

    #[test]
    fn an_unmanaged_session_refuses_stop() {
        let stranger = ServiceFixture::session("mystery");
        let fixture = ServiceFixture::new().with_multiplexer(
            FakeMultiplexer::new().with_live(ServiceFixture::waiting(&stranger).without_metadata()),
        );

        let error = fixture.service().stop_session("mystery").unwrap_err();

        assert!(matches!(error, ServiceError::Conflict(_)));
    }

    #[test]
    fn the_session_that_runs_this_ringleader_refuses_stop() {
        let fixture = StopFixture::stopped().with_multiplexer(
            FakeMultiplexer::new()
                .with_live(ServiceFixture::waiting(&StopFixture::session()))
                .with_caller("ringleader-fix-auth"),
        );

        let error = StopFixture::stop(&fixture).unwrap_err();

        assert!(
            error.to_string().contains("runs this Ringleader"),
            "{error}"
        );
    }

    #[test]
    fn a_process_that_survives_the_stop_is_named_in_the_refusal() {
        let session = StopFixture::session();
        let host = ServiceFixture::host_for(&[&session]).with_stubborn_process(
            StopFixture::DEV_SERVER,
            "npm run dev",
            session.worktree_path(),
            StopFixture::PANE_SESSION,
        );
        let fixture = StopFixture::waiting().with_host(host);

        let error = StopFixture::stop(&fixture).unwrap_err();

        assert!(error.to_string().contains("npm run dev"), "{error}");
        assert!(fixture.multiplexer().live_names().is_empty());
    }
}
