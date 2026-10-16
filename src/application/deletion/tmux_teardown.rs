use std::sync::Arc;

use crate::application::ServiceError;
use crate::application::ServicePorts;
use crate::application::ports::{HostProbe, Multiplexer, ProcessInfo};
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

    pub fn kill(&self, row: &ManagedRow, kept: &str) -> Result<Vec<u32>, ServiceError> {
        if !row.is_live() {
            return Ok(Vec::new());
        }
        let keep = |error| ServiceError::from(error).adding_context(kept);
        let tmux_name = row.session().tmux_name();
        let pane_sessions = self.multiplexer.pane_pids(tmux_name).map_err(keep)?;
        self.multiplexer.kill_session(tmux_name).map_err(keep)?;
        Ok(self.left_in(&pane_sessions))
    }

    fn left_in(&self, pane_sessions: &[u32]) -> Vec<u32> {
        self.host
            .session_processes(pane_sessions)
            .iter()
            .map(ProcessInfo::pid)
            .collect()
    }
}
