use std::sync::Arc;

use super::busy_worktree::BusyWorktree;
use super::ports::{HostProbe, Multiplexer};
use super::reconciliation::ManagedRow;
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use crate::domain::SessionState;

pub struct UnseenSession {
    multiplexer: Arc<dyn Multiplexer>,
    host: Arc<dyn HostProbe>,
}

impl UnseenSession {
    const RENAME_WAY_OUT: &'static str = "If that tmux session is already gone, restart this session first, which records it on Ringleader's tmux server.";

    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            multiplexer: ports.tmux().multiplexer(),
            host: ports.repos().host(),
        }
    }

    pub fn refuse_rename(&self, row: &ManagedRow) -> Result<(), ServiceError> {
        self.refuse_behind_lost_socket(row)?;
        match self.on_recorded_server(row) {
            Some(finding) => Err(ServiceError::Conflict(format!(
                "{finding} {}",
                Self::RENAME_WAY_OUT
            ))),
            None => Ok(()),
        }
    }

    pub fn refuse_behind_lost_socket(&self, row: &ManagedRow) -> Result<(), ServiceError> {
        if !Self::is_stopped(row) || !self.multiplexer.socket_lost() {
            return Ok(());
        }
        Err(ServiceError::Conflict(format!(
            "session {} shows as stopped, but {}",
            row.session().name(),
            BusyWorktree::lost_socket_advice(&self.multiplexer.socket())
        )))
    }

    pub fn on_recorded_server(&self, row: &ManagedRow) -> Option<String> {
        let session = row.session();
        let recorded = session.socket_path();
        let unseen = Self::is_stopped(row)
            && recorded != self.multiplexer.socket()
            && self.host.tmux_server_serves(recorded);
        unseen.then(|| {
            format!(
                "the tmux server at {} still runs and may hold the tmux session of {}. {}",
                recorded.display(),
                session.name(),
                BusyWorktree::recorded_server_advice(session)
            )
        })
    }

    fn is_stopped(row: &ManagedRow) -> bool {
        row.state() == SessionState::Stopped
    }
}
