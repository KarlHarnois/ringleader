use std::sync::Arc;

use crate::application::ServiceError;
use crate::application::ServicePorts;
use crate::application::ports::{HostProbe, Multiplexer, MultiplexerError, ProcessInfo};
use crate::application::reconciliation::ManagedRow;

pub struct TmuxTeardown {
    multiplexer: Arc<dyn Multiplexer>,
    host: Arc<dyn HostProbe>,
}

impl TmuxTeardown {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            multiplexer: ports.tmux().multiplexer(),
            host: ports.repos().host(),
        }
    }

    pub fn pane_sessions(&self, row: &ManagedRow, kept: &str) -> Result<Vec<u32>, ServiceError> {
        if !row.is_live() {
            return Ok(Vec::new());
        }
        self.multiplexer
            .pane_pids(row.session().tmux_name())
            .map_err(Self::adding(kept))
    }

    pub fn pane_processes(&self, pane_sessions: &[u32]) -> Vec<u32> {
        self.host
            .session_processes(pane_sessions)
            .iter()
            .map(ProcessInfo::pid)
            .collect()
    }

    pub fn kill(&self, row: &ManagedRow, kept: &str) -> Result<(), ServiceError> {
        if !row.is_live() {
            return Ok(());
        }
        self.multiplexer
            .kill_session(row.session().tmux_name())
            .map_err(Self::adding(kept))
    }

    fn adding(kept: &str) -> impl Fn(MultiplexerError) -> ServiceError {
        move |error| ServiceError::from(error).adding_context(kept)
    }
}
