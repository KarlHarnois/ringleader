use crate::application::{
    DeleteConfirmation, DeleteImpact, DeleteOutcome, ServiceError, SessionService,
};
use crate::ui::operation_outcome::OperationOutcome;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteStep {
    Measure(String),
    Confirmed { name: String, seen: DeleteImpact },
}

impl DeleteStep {
    pub fn busy_message(&self) -> String {
        match self {
            Self::Measure(name) => format!("Checking what deleting {name} would lose…"),
            Self::Confirmed { name, .. } => format!("Deleting {name}…"),
        }
    }

    pub fn run(self, service: &mut SessionService) -> OperationOutcome {
        match self {
            Self::Measure(name) => Self::measure(service, name),
            Self::Confirmed { name, seen } => Self::delete(service, name, seen),
        }
    }

    fn measure(service: &mut SessionService, name: String) -> OperationOutcome {
        match service.delete_impact(&name) {
            Ok(impact) => OperationOutcome::ConfirmDelete {
                name,
                impact,
                changed: false,
            },
            Err(error @ ServiceError::Unmeasurable(_)) => OperationOutcome::Failed(error.adding_context(&format!(
                "ringleader delete {name} --forget needs this TUI to quit first, since the TUI holds the lock"
            ))),
            Err(error) => OperationOutcome::Failed(error),
        }
    }

    fn delete(service: &mut SessionService, name: String, seen: DeleteImpact) -> OperationOutcome {
        match service.delete_session(&name, &DeleteConfirmation::confirmed(seen)) {
            Ok(outcome) => Self::deleted(&name, &outcome),
            Err(ServiceError::GuardsChanged { impact, .. }) => OperationOutcome::ConfirmDelete {
                name,
                impact: *impact,
                changed: true,
            },
            Err(error) => OperationOutcome::Failed(error),
        }
    }

    fn deleted(name: &str, outcome: &DeleteOutcome) -> OperationOutcome {
        let summary = outcome.summary(name);
        match outcome.stray_session() {
            Some(warning) => OperationOutcome::Warned {
                message: format!("{summary}; {warning}"),
                select: None,
            },
            None => OperationOutcome::done(summary, None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ServiceFixture;

    struct MeasureFixture;

    impl MeasureFixture {
        const NAME: &'static str = "fix-auth";

        fn stopped() -> ServiceFixture {
            let session = ServiceFixture::session(Self::NAME);
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&session))
                .with_workspace(ServiceFixture::workspace_for(&[&session]))
                .with_host(ServiceFixture::host_for(&[&session]))
        }
    }

    #[test]
    fn a_first_measurement_asks_to_confirm_without_saying_the_impact_changed() {
        let fixture = MeasureFixture::stopped();

        let outcome =
            DeleteStep::Measure(MeasureFixture::NAME.to_string()).run(&mut fixture.service());

        assert!(matches!(
            outcome,
            OperationOutcome::ConfirmDelete { changed: false, .. }
        ));
    }
}
