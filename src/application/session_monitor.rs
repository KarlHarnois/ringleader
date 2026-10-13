use std::sync::Arc;

use super::ports::{Clock, DiffBase, HostProbe, Multiplexer, Workspace};
use super::reconciliation::Reconciliation;
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use super::session_reconciler::SessionReconciler;
use crate::domain::{Config, DiffStat, ReturnKey, Session};

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

    pub fn reconcile(
        &self,
        entries: &[Session],
        config: &Config,
    ) -> Result<Reconciliation, ServiceError> {
        let observations = self.multiplexer.observe_all()?;
        let now = self.clock.now_epoch_seconds();
        let reconciler = SessionReconciler::new(config, self.host.as_ref(), now);
        Ok(reconciler.reconcile(entries, observations))
    }

    pub fn diff_stat(&self, session: &Session) -> Result<DiffStat, ServiceError> {
        let base = DiffBase::new(session.base_commit(), None);
        Ok(self.workspace.diff_stat(session.worktree_path(), &base)?)
    }

    pub fn preview(&self, tmux_name: &str) -> Result<String, ServiceError> {
        Ok(self.multiplexer.capture_pane(tmux_name)?)
    }

    pub fn server_return_key(&self) -> Option<ReturnKey> {
        let running = self
            .multiplexer
            .server_config(&ReturnKey::default())
            .ok()??;
        running.return_key().cloned()
    }
}
