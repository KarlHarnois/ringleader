use std::sync::Arc;

use crate::application::ServiceError;
use crate::application::ServicePorts;
use crate::application::ports::{HostProbe, Multiplexer, ProcessInfo};
use crate::application::reconciliation::ManagedRow;

pub struct TmuxTeardown {
    multiplexer: Arc<dyn Multiplexer>,
    host: Arc<dyn HostProbe>,
    kept: TeardownContexts,
}

pub struct TeardownContexts {
    on_tmux_failure: &'static str,
    while_running: &'static str,
    after_stop: &'static str,
}

impl TeardownContexts {
    pub const fn new(
        on_tmux_failure: &'static str,
        while_running: &'static str,
        after_stop: &'static str,
    ) -> Self {
        Self {
            on_tmux_failure,
            while_running,
            after_stop,
        }
    }
}

impl TmuxTeardown {
    pub fn new(ports: &ServicePorts, kept: TeardownContexts) -> Self {
        Self {
            multiplexer: ports.tmux().multiplexer(),
            host: ports.repos().host(),
            kept,
        }
    }

    pub fn stop(
        &self,
        row: &ManagedRow,
        confirmed: &[u32],
    ) -> Result<Vec<ProcessInfo>, ServiceError> {
        if row.is_live() {
            return self.stop_live(row, confirmed);
        }
        self.end(confirmed)
    }

    fn stop_live(
        &self,
        row: &ManagedRow,
        confirmed: &[u32],
    ) -> Result<Vec<ProcessInfo>, ServiceError> {
        let pane_sessions = self.pane_sessions(row)?;
        self.refuse_wrong_batch(&pane_sessions, confirmed)?;
        self.kill(row)?;
        self.end(&self.processes_to_end(&pane_sessions, confirmed))
    }

    fn pane_sessions(&self, row: &ManagedRow) -> Result<Vec<u32>, ServiceError> {
        self.multiplexer
            .pane_pids(row.session().tmux_name())
            .map_err(Self::adding(self.kept.on_tmux_failure))
    }

    fn refuse_wrong_batch(
        &self,
        pane_sessions: &[u32],
        confirmed: &[u32],
    ) -> Result<(), ServiceError> {
        self.host
            .check_batch(&self.processes_to_end(pane_sessions, confirmed))
            .map_err(Self::adding(self.kept.while_running))
    }

    fn kill(&self, row: &ManagedRow) -> Result<(), ServiceError> {
        self.multiplexer
            .kill_session(row.session().tmux_name())
            .map_err(Self::adding(self.kept.on_tmux_failure))
    }

    fn end(&self, pids: &[u32]) -> Result<Vec<ProcessInfo>, ServiceError> {
        self.host
            .terminate(pids)
            .map_err(Self::adding(self.kept.after_stop))
    }

    fn processes_to_end(&self, pane_sessions: &[u32], confirmed: &[u32]) -> Vec<u32> {
        let mut batch: Vec<u32> = self
            .host
            .session_processes(pane_sessions)
            .iter()
            .map(ProcessInfo::pid)
            .collect();
        batch.extend_from_slice(confirmed);
        batch
    }

    fn adding<E: Into<ServiceError>>(kept: &str) -> impl Fn(E) -> ServiceError {
        move |error| error.into().adding_context(kept)
    }
}
