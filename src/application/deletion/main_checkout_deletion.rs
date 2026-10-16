use std::sync::Arc;

use super::delete_impact::{DeleteConfirmation, DeleteImpact};
use super::delete_outcome::DeleteOutcome;
use crate::application::ServiceError;
use crate::application::ServicePorts;
use crate::application::checks::unseen_session::UnseenSession;
use crate::application::ports::{Multiplexer, SessionRegistry};
use crate::application::reconciliation::ManagedRow;
use crate::application::session_scratch::SessionScratch;
use crate::domain::{Session, SessionState};

pub struct MainCheckoutDeletion {
    unseen: UnseenSession,
    multiplexer: Arc<dyn Multiplexer>,
    registry: Arc<dyn SessionRegistry>,
    scratch: SessionScratch,
}

impl MainCheckoutDeletion {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            unseen: UnseenSession::new(ports),
            multiplexer: ports.tmux().multiplexer(),
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
        self.kill_tmux_session(row)?;
        self.scratch.discard(row.session().worktree_path());
        self.registry.save(remaining)?;
        let stray_session = self.unseen.stray_session_warning(row);
        Ok(DeleteOutcome::checkout_kept(row.session()).with_stray_session(stray_session))
    }

    fn kill_tmux_session(&self, row: &ManagedRow) -> Result<(), ServiceError> {
        if !row.is_live() {
            return Ok(());
        }
        self.multiplexer
            .kill_session(row.session().tmux_name())
            .map_err(|error| {
                ServiceError::from(error)
                    .adding_context("kept the registry entry, run ringleader doctor")
            })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::application::SessionService;
    use crate::domain::SessionObservation;
    use crate::test_support::{FakeMultiplexer, ServiceFixture};

    struct CheckoutDeleteFixture;

    impl CheckoutDeleteFixture {
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
