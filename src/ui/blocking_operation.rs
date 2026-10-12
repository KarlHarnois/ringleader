use crate::application::{
    CreateRequest, DeleteConfirmation, DeleteImpact, ServiceError, SessionService,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockingOperation {
    Restart(String),
    Create(CreateRequest),
    MeasureDelete(String),
    Delete { name: String, seen: DeleteImpact },
    Rename { old: String, new: String },
}

impl BlockingOperation {
    pub fn busy_message(&self) -> String {
        match self {
            Self::Restart(name) => format!("Restarting {name}…"),
            Self::Create(request) => format!("Creating {}…", request.name()),
            Self::MeasureDelete(name) => format!("Checking what deleting {name} would lose…"),
            Self::Delete { name, .. } => format!("Deleting {name}…"),
            Self::Rename { old, .. } => format!("Renaming {old}…"),
        }
    }

    pub fn run(self, service: &mut SessionService) -> OperationOutcome {
        match self {
            Self::Restart(name) => match service.restart_session(&name) {
                Ok(()) => OperationOutcome::done(format!("Restarted {name}."), Some(name)),
                Err(error) => OperationOutcome::Failed(error),
            },
            Self::Create(request) => match service.create_session(&request) {
                Ok(session) => OperationOutcome::done(
                    format!("Created {} on branch {}.", session.name(), session.branch()),
                    Some(session.name().to_string()),
                ),
                Err(error) => OperationOutcome::Failed(error),
            },
            Self::MeasureDelete(name) => Self::measure(service, name),
            Self::Delete { name, seen } => Self::delete(service, name, seen),
            Self::Rename { old, new } => match service.rename_session(&old, &new) {
                Ok(session) => OperationOutcome::done(
                    format!("Renamed {old} to {}.", session.name()),
                    Some(session.name().to_string()),
                ),
                Err(error) => OperationOutcome::Failed(error),
            },
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
            Ok(outcome) => OperationOutcome::done(outcome.summary(&name), None),
            Err(ServiceError::GuardsChanged { impact, .. }) => OperationOutcome::ConfirmDelete {
                name,
                impact: *impact,
                changed: true,
            },
            Err(error) => OperationOutcome::Failed(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationOutcome {
    Done {
        message: String,
        select: Option<String>,
    },
    ConfirmDelete {
        name: String,
        impact: DeleteImpact,
        changed: bool,
    },
    Failed(ServiceError),
}

impl OperationOutcome {
    fn done(message: String, select: Option<String>) -> Self {
        Self::Done { message, select }
    }
}
