use std::sync::Arc;

use super::ports::{Multiplexer, MultiplexerError};
use super::service_ports::ServicePorts;
use crate::domain::{Agent, Session};

pub struct AgentHooks {
    multiplexer: Arc<dyn Multiplexer>,
}

impl AgentHooks {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            multiplexer: ports.tmux().multiplexer(),
        }
    }

    pub fn after_start(&self, session: &Session, agent: &Agent) {
        let lines = agent.startup_input(session.name());
        if lines.is_empty() {
            return;
        }
        let typed = self
            .multiplexer
            .type_when_ready(session.tmux_name(), &lines);
        Self::log_skipped(session, typed);
    }

    pub fn after_rename(&self, renamed: &Session, agent: &Agent) {
        let lines = agent.rename_input(renamed.name());
        if lines.is_empty() {
            return;
        }
        let typed = self.multiplexer.type_if_ready(renamed.tmux_name(), &lines);
        Self::log_skipped(renamed, typed);
    }

    fn log_skipped(session: &Session, typed: Result<(), MultiplexerError>) {
        let tmux_name = session.tmux_name();
        match typed {
            Ok(()) => {}
            Err(error @ MultiplexerError::AgentNotReady { .. }) => {
                log::info!("skipped typing into {tmux_name}: {error}");
            }
            Err(error) => log::warn!("could not type into {tmux_name}: {error}"),
        }
    }
}
