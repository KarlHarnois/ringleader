use crate::domain::Session;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartOutcome {
    session: Session,
    warning: Option<String>,
    recovery: Option<String>,
}

impl StartOutcome {
    pub fn new(session: Session, warning: Option<String>) -> Self {
        Self {
            session,
            warning,
            recovery: None,
        }
    }

    pub fn with_recovery(self, recovery: Option<String>) -> Self {
        Self { recovery, ..self }
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    pub fn recovery(&self) -> Option<&str> {
        self.recovery.as_deref()
    }
}
