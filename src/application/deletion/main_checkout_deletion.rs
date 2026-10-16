use std::sync::Arc;

use super::delete_impact::{DeleteConfirmation, DeleteImpact};
use super::delete_outcome::DeleteOutcome;
use super::tmux_teardown::TmuxTeardown;
use crate::application::ServiceError;
use crate::application::ServicePorts;
use crate::application::checks::unseen_session::UnseenSession;
use crate::application::ports::{HostProbe, ProcessInfo, SessionRegistry};
use crate::application::reconciliation::ManagedRow;
use crate::application::session_scratch::SessionScratch;
use crate::domain::{Session, SessionState};

pub struct MainCheckoutDeletion {
    unseen: UnseenSession,
    teardown: TmuxTeardown,
    host: Arc<dyn HostProbe>,
    registry: Arc<dyn SessionRegistry>,
    scratch: SessionScratch,
}

impl MainCheckoutDeletion {
    const KEPT_ON_TMUX_FAILURE: &'static str = "kept the registry entry, run ringleader doctor";

    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            unseen: UnseenSession::new(ports),
            teardown: TmuxTeardown::new(ports),
            host: ports.repos().host(),
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
        let started_by_the_agent = self.teardown.kill(row, Self::KEPT_ON_TMUX_FAILURE)?;
        let survivors = self.host.terminate(&started_by_the_agent);
        if survivors.is_empty() {
            return Ok(());
        }
        Err(ServiceError::Conflict(format!(
            "processes the agent started still run: {}; end them by hand, since the session is now stopped and a second delete cannot find them",
            ProcessInfo::listed(&survivors)
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
        let host = CheckoutDeleteFixture::agent_host().with_process(
            CheckoutDeleteFixture::DEV_SERVER,
            "npm run dev",
            CheckoutDeleteFixture::checkout(),
            CheckoutDeleteFixture::PANE_SESSION,
        );
        let fixture = CheckoutDeleteFixture::with_agent_process(host);

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
            impact.warnings(),
            ["the main checkout and its branch stay, delete ends only the session"]
        );
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
