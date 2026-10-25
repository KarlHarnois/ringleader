use std::sync::Arc;

use super::delete_impact::{DeleteConfirmation, DeleteImpact};
use super::delete_outcome::DeleteOutcome;
use crate::application::ServiceError;
use crate::application::ServicePorts;
use crate::application::checks::unseen_session::UnseenSession;
use crate::application::ports::{ProcessInfo, SessionRegistry};
use crate::application::reconciliation::ManagedRow;
use crate::application::session_scratch::SessionScratch;
use crate::application::tmux_teardown::{TeardownContexts, TmuxTeardown};
use crate::domain::{Session, SessionState};

pub struct MainCheckoutDeletion {
    unseen: UnseenSession,
    teardown: TmuxTeardown,
    registry: Arc<dyn SessionRegistry>,
    scratch: SessionScratch,
}

impl MainCheckoutDeletion {
    const KEPT_ON_TMUX_FAILURE: &'static str = "kept the registry entry, run ringleader doctor";
    const KEPT_WHILE_RUNNING: &'static str =
        "kept the registry entry and the session, which still runs";
    const SESSION_STOPPED: &'static str =
        "the session is now stopped and a second delete cannot find them";
    const TEARDOWN_CONTEXTS: TeardownContexts = TeardownContexts::new(
        Self::KEPT_ON_TMUX_FAILURE,
        Self::KEPT_WHILE_RUNNING,
        Self::SESSION_STOPPED,
    );

    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            unseen: UnseenSession::new(ports),
            teardown: TmuxTeardown::new(ports, Self::TEARDOWN_CONTEXTS),
            registry: ports.state().registry(),
            scratch: SessionScratch::new(ports),
        }
    }

    pub fn impact(&self, row: &ManagedRow) -> Result<DeleteImpact, ServiceError> {
        self.unseen.refuse_behind_lost_socket(row)?;
        Ok(DeleteImpact::session_only().while_working(row.state() == SessionState::Working))
    }

    pub fn delete(
        &self,
        row: &ManagedRow,
        remaining: &[Session],
        confirmation: &DeleteConfirmation,
    ) -> Result<DeleteOutcome, ServiceError> {
        confirmation.check(&self.impact(row)?)?;
        self.end_tmux_session(row)?;
        self.scratch.discard(row.session().worktree_path());
        self.registry.save(remaining)?;
        let stray_session = self.unseen.stray_session_warning(row);
        Ok(DeleteOutcome::checkout_kept(row.session()).with_stray_session(stray_session))
    }

    fn end_tmux_session(&self, row: &ManagedRow) -> Result<(), ServiceError> {
        let survivors = self.teardown.stop(row, &[])?;
        Self::refuse_survivors(&survivors)
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
    use std::path::Path;

    use super::*;
    use crate::application::SessionService;
    use crate::domain::SessionObservation;
    use crate::test_support::{FakeHostProbe, FakeMultiplexer, ServiceFixture};

    struct CheckoutDeleteFixture;

    impl CheckoutDeleteFixture {
        const PANE_SESSION: u32 = 4100;
        const DEV_SERVER: u32 = 4200;

        fn session() -> Session {
            ServiceFixture::main_checkout_session("fix-auth")
        }

        fn stopped() -> ServiceFixture {
            let session = Self::session();
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&session))
                .with_workspace(ServiceFixture::main_checkout_workspace())
                .with_host(ServiceFixture::with_worktree_process(&session))
        }

        fn waiting() -> ServiceFixture {
            Self::live(ServiceFixture::waiting(&Self::session()))
        }

        fn with_agent_process(host: FakeHostProbe) -> ServiceFixture {
            let session = Self::session();
            let multiplexer = FakeMultiplexer::new()
                .with_live(ServiceFixture::waiting(&session))
                .with_pane_pids(session.tmux_name(), &[Self::PANE_SESSION]);
            Self::stopped()
                .with_host(host)
                .with_multiplexer(multiplexer)
        }

        fn agent_host() -> FakeHostProbe {
            ServiceFixture::host_for(&[&Self::session()])
        }

        fn dev_server_host() -> FakeHostProbe {
            Self::agent_host().with_process(
                Self::DEV_SERVER,
                "npm run dev",
                Self::checkout(),
                Self::PANE_SESSION,
            )
        }

        fn refusing_dev_server() -> ServiceFixture {
            Self::with_agent_process(
                Self::dev_server_host().refusing_to_terminate(Self::DEV_SERVER),
            )
        }

        fn live(observation: SessionObservation) -> ServiceFixture {
            Self::stopped().with_multiplexer(FakeMultiplexer::new().with_live(observation))
        }

        fn force(fixture: &ServiceFixture) -> Result<DeleteOutcome, ServiceError> {
            Self::delete(&mut fixture.service(), &DeleteConfirmation::force())
        }

        fn delete(
            service: &mut SessionService,
            confirmation: &DeleteConfirmation,
        ) -> Result<DeleteOutcome, ServiceError> {
            service.delete_session("fix-auth", confirmation)
        }

        fn checkout() -> &'static Path {
            Path::new(ServiceFixture::REPO)
        }
    }

    #[test]
    fn deleting_a_session_in_the_main_checkout_ends_its_tmux_session() {
        let fixture = CheckoutDeleteFixture::waiting();

        CheckoutDeleteFixture::force(&fixture).unwrap();

        assert!(fixture.multiplexer().live_names().is_empty());
    }

    #[test]
    fn deleting_a_session_in_the_main_checkout_forgets_it() {
        let fixture = CheckoutDeleteFixture::waiting();

        CheckoutDeleteFixture::force(&fixture).unwrap();

        assert!(fixture.registry().sessions().is_empty());
    }

    #[test]
    fn deleting_a_session_in_the_main_checkout_changes_nothing_in_git() {
        let fixture = CheckoutDeleteFixture::waiting();

        CheckoutDeleteFixture::force(&fixture).unwrap();

        assert!(fixture.workspace().state_changing_calls().is_empty());
    }

    #[test]
    fn deleting_a_session_in_the_main_checkout_never_stops_the_users_processes_there() {
        let fixture = CheckoutDeleteFixture::waiting();

        CheckoutDeleteFixture::force(&fixture).unwrap();

        assert!(fixture.host().terminated().is_empty());
    }

    #[test]
    fn deleting_a_session_in_the_main_checkout_stops_the_processes_its_agent_started() {
        let fixture =
            CheckoutDeleteFixture::with_agent_process(CheckoutDeleteFixture::dev_server_host());

        CheckoutDeleteFixture::force(&fixture).unwrap();

        assert_eq!(
            fixture.host().terminated(),
            [CheckoutDeleteFixture::DEV_SERVER]
        );
    }

    #[test]
    fn a_process_its_agent_started_that_survives_keeps_the_session_recorded() {
        let host = CheckoutDeleteFixture::agent_host().with_stubborn_process(
            CheckoutDeleteFixture::DEV_SERVER,
            "npm run dev",
            CheckoutDeleteFixture::checkout(),
            CheckoutDeleteFixture::PANE_SESSION,
        );
        let fixture = CheckoutDeleteFixture::with_agent_process(host);

        let error = CheckoutDeleteFixture::force(&fixture).unwrap_err();

        assert!(error.to_string().contains("npm run dev"));
        assert_eq!(fixture.registry().sessions().len(), 1);
    }

    #[test]
    fn a_process_its_agent_started_that_survives_is_left_for_the_user_to_end_by_hand() {
        let host = CheckoutDeleteFixture::agent_host().with_stubborn_process(
            CheckoutDeleteFixture::DEV_SERVER,
            "npm run dev",
            CheckoutDeleteFixture::checkout(),
            CheckoutDeleteFixture::PANE_SESSION,
        );
        let fixture = CheckoutDeleteFixture::with_agent_process(host);

        let error = CheckoutDeleteFixture::force(&fixture).unwrap_err();

        assert!(error.to_string().contains("end them by hand"));
    }

    #[test]
    fn a_batch_the_host_refuses_to_end_leaves_the_session_live_and_recorded() {
        let fixture = CheckoutDeleteFixture::refusing_dev_server();

        let error = CheckoutDeleteFixture::force(&fixture).unwrap_err();

        assert!(matches!(error, ServiceError::Conflict(_)), "{error}");
        assert!(error.to_string().contains("end the processes by hand"));
        assert!(error.to_string().contains("which still runs"));
        assert_eq!(fixture.multiplexer().live_names(), ["ringleader-fix-auth"]);
        assert_eq!(fixture.registry().sessions().len(), 1);
    }

    #[test]
    fn a_batch_the_host_refuses_to_end_signals_nothing() {
        let fixture = CheckoutDeleteFixture::refusing_dev_server();

        CheckoutDeleteFixture::force(&fixture).unwrap_err();

        assert!(fixture.host().terminated().is_empty());
    }

    #[test]
    fn the_processes_its_agent_started_are_ended_only_after_the_tmux_session_is_killed() {
        let fixture =
            CheckoutDeleteFixture::with_agent_process(CheckoutDeleteFixture::dev_server_host());
        let before_kill = fixture.terminated_before("kill_session");

        CheckoutDeleteFixture::force(&fixture).unwrap();

        assert!(before_kill.pids().is_empty());
        assert_eq!(
            fixture.host().terminated(),
            [CheckoutDeleteFixture::DEV_SERVER]
        );
    }

    #[test]
    fn a_refusal_the_kill_reveals_says_the_session_is_now_stopped() {
        let fixture =
            CheckoutDeleteFixture::with_agent_process(CheckoutDeleteFixture::dev_server_host());
        fixture.refuse_to_terminate_at("kill_session", CheckoutDeleteFixture::DEV_SERVER);

        let error = CheckoutDeleteFixture::force(&fixture).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("a second delete cannot find them"),
            "{error}"
        );
        assert!(fixture.multiplexer().live_names().is_empty());
        assert_eq!(fixture.registry().sessions().len(), 1);
    }

    #[test]
    fn deleting_a_session_in_the_main_checkout_discards_its_scratch_directory() {
        let fixture = CheckoutDeleteFixture::waiting();

        CheckoutDeleteFixture::force(&fixture).unwrap();

        assert_eq!(
            fixture.scratch_space().removed(),
            [CheckoutDeleteFixture::checkout()]
        );
    }

    #[test]
    fn a_session_in_the_main_checkout_is_deleted_from_a_ringleader_launched_there() {
        let fixture =
            CheckoutDeleteFixture::waiting().launched_in(CheckoutDeleteFixture::checkout());

        assert!(CheckoutDeleteFixture::force(&fixture).is_ok());
    }

    #[test]
    fn a_working_session_in_the_main_checkout_needs_confirmation_to_delete() {
        let fixture =
            CheckoutDeleteFixture::live(ServiceFixture::working(&CheckoutDeleteFixture::session()));
        let unconfirmed = DeleteConfirmation::confirmed(DeleteImpact::default());

        let error =
            CheckoutDeleteFixture::delete(&mut fixture.service(), &unconfirmed).unwrap_err();

        assert!(matches!(error, ServiceError::GuardsChanged { .. }));
        assert_eq!(fixture.registry().sessions().len(), 1);
    }

    #[test]
    fn the_delete_impact_of_a_session_in_the_main_checkout_says_the_checkout_stays() {
        let fixture = CheckoutDeleteFixture::waiting();

        let impact = fixture.service().delete_impact("fix-auth").unwrap();

        assert_eq!(
            impact.kept(),
            Some("the main checkout and its branch stay, delete ends only the session")
        );
    }

    #[test]
    fn a_working_session_in_the_main_checkout_lists_only_its_working_agent_as_lost() {
        let fixture =
            CheckoutDeleteFixture::live(ServiceFixture::working(&CheckoutDeleteFixture::session()));

        let impact = fixture.service().delete_impact("fix-auth").unwrap();

        assert_eq!(impact.warnings(), ["agent is actively working"]);
    }

    #[test]
    fn a_stopped_session_in_the_main_checkout_behind_a_lost_socket_refuses_the_delete() {
        let fixture = CheckoutDeleteFixture::stopped()
            .with_multiplexer(FakeMultiplexer::new().with_lost_socket());

        let error = CheckoutDeleteFixture::force(&fixture).unwrap_err();

        assert!(matches!(error, ServiceError::Conflict(_)));
        assert_eq!(fixture.registry().sessions().len(), 1);
    }

    #[test]
    fn deleting_a_session_in_the_main_checkout_names_what_it_kept() {
        let fixture = CheckoutDeleteFixture::waiting();

        let outcome = CheckoutDeleteFixture::force(&fixture).unwrap();

        assert_eq!(
            outcome.summary("fix-auth"),
            format!(
                "deleted fix-auth, kept the main checkout {} and its branch main",
                ServiceFixture::REPO
            )
        );
    }
}
