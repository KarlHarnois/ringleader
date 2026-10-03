use crate::application::{CreateRequest, SessionService};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockingOperation {
    Restart(String),
    Create(CreateRequest),
}

impl BlockingOperation {
    pub fn busy_message(&self) -> String {
        match self {
            Self::Restart(name) => format!("Restarting {name}…"),
            Self::Create(request) => format!("Creating {}…", request.name()),
        }
    }

    pub fn run(&self, service: &mut SessionService) -> Result<OperationDone, String> {
        match self {
            Self::Restart(name) => service
                .restart_session(name)
                .map(|()| OperationDone::new(format!("Restarted {name}."), name)),
            Self::Create(request) => service.create_session(request).map(|session| {
                let message = format!("Created {} on branch {}.", session.name(), session.branch());
                OperationDone::new(message, session.name().as_str())
            }),
        }
        .map_err(|error| error.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationDone {
    message: String,
    session: String,
}

impl OperationDone {
    fn new(message: String, session: &str) -> Self {
        Self {
            message,
            session: session.to_string(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn session(&self) -> &str {
        &self.session
    }
}
