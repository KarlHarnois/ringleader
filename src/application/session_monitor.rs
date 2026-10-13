use std::sync::Arc;

use super::pending::Pending;
use super::ports::{Clock, HostProbe, Multiplexer, Workspace};
use super::reconciliation::Reconciliation;
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use super::session_reconciler::SessionReconciler;
use crate::domain::{Config, DiffStat, ReturnKey, Session, SessionObservation};

pub struct SessionMonitor {
    multiplexer: Arc<dyn Multiplexer>,
    workspace: Arc<dyn Workspace>,
    host: Arc<dyn HostProbe>,
    clock: Arc<dyn Clock>,
}

impl SessionMonitor {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            multiplexer: ports.tmux().multiplexer(),
            workspace: ports.repos().workspace(),
            host: ports.repos().host(),
            clock: ports.state().clock(),
        }
    }

    pub fn observe_all(&self) -> Result<Vec<SessionObservation>, ServiceError> {
        Ok(self.multiplexer.observe_all()?)
    }

    pub fn start_observing(&self) -> Pending<Vec<SessionObservation>> {
        let multiplexer = Arc::clone(&self.multiplexer);
        Pending::start(move || Ok(multiplexer.observe_all()?))
    }

    pub fn reconcile(
        &self,
        entries: &[Session],
        config: &Config,
        observations: Vec<SessionObservation>,
    ) -> Reconciliation {
        let now = self.clock.now_epoch_seconds();
        let reconciler = SessionReconciler::new(config, self.host.as_ref(), now);
        reconciler.reconcile(entries, observations)
    }

    pub fn diff_stat(&self, session: &Session) -> Result<DiffStat, ServiceError> {
        Ok(self
            .workspace
            .diff_stat(session.worktree_path(), session.base_commit())?)
    }

    pub fn start_diff(&self, session: &Session) -> Pending<DiffStat> {
        let workspace = Arc::clone(&self.workspace);
        let worktree = session.worktree_path().to_path_buf();
        let base_commit = session.base_commit().to_string();
        Pending::start(move || Ok(workspace.diff_stat(&worktree, &base_commit)?))
    }

    pub fn start_preview(&self, tmux_name: &str) -> Pending<String> {
        let multiplexer = Arc::clone(&self.multiplexer);
        let tmux_name = tmux_name.to_string();
        Pending::start(move || Ok(multiplexer.capture_pane(&tmux_name)?))
    }

    pub fn server_return_key(&self) -> Option<ReturnKey> {
        let running = self
            .multiplexer
            .server_config(&ReturnKey::default())
            .ok()??;
        running.return_key().cloned()
    }
}
