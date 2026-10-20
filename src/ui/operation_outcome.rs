use crate::application::{DeleteImpact, ServiceError, StartOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationOutcome {
    Done {
        message: String,
        select: Option<String>,
    },
    Warned {
        message: String,
        select: Option<String>,
    },
    ConfirmDelete {
        name: String,
        impact: DeleteImpact,
        changed: bool,
    },
    Opened(String),
    Failed(ServiceError),
}

impl OperationOutcome {
    pub fn done(message: String, select: Option<String>) -> Self {
        Self::Done { message, select }
    }

    pub fn started(message: String, outcome: &StartOutcome) -> Self {
        let select = Some(outcome.session().name().to_string());
        match outcome.warning() {
            Some(warning) => Self::Warned {
                message: format!("{message} But {warning}."),
                select,
            },
            None => Self::done(message, select),
        }
    }

    pub fn may_have_changed_sessions(&self) -> bool {
        !matches!(self, Self::Opened(_))
    }
}
