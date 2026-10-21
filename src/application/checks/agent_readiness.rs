use crate::application::ServiceError;
use crate::application::ports::Multiplexer;
use crate::domain::{Session, SessionName};

pub struct AgentReadiness<'a> {
    multiplexer: &'a dyn Multiplexer,
}

impl<'a> AgentReadiness<'a> {
    pub fn new(multiplexer: &'a dyn Multiplexer) -> Self {
        Self { multiplexer }
    }

    pub fn refuse_unless_at_prompt(
        &self,
        session: &Session,
        action: &str,
    ) -> Result<(), ServiceError> {
        if self.multiplexer.agent_at_prompt(session.tmux_name())? {
            return Ok(());
        }
        Err(Self::not_at_prompt(session.name(), action))
    }

    pub fn not_at_prompt(name: &SessionName, action: &str) -> ServiceError {
        ServiceError::Conflict(format!(
            "the agent of session {name} is not at its prompt, answer its dialog, send or clear its draft, or leave copy mode before {action}"
        ))
    }
}
