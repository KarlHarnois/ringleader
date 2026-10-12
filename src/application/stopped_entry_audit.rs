use std::sync::Arc;

use super::busy_worktree::BusyWorktree;
use super::doctor_report::DoctorReport;
use super::ports::{HostProbe, Multiplexer};
use super::reconciliation::Reconciliation;
use super::service_ports::ServicePorts;
use crate::domain::Session;

pub struct StoppedEntryAudit {
    multiplexer: Arc<dyn Multiplexer>,
    host: Arc<dyn HostProbe>,
}

impl StoppedEntryAudit {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            multiplexer: ports.tmux().multiplexer(),
            host: ports.repos().host(),
        }
    }

    pub fn audit(
        &self,
        session: &Session,
        reconciliation: &Reconciliation,
        report: &mut DoctorReport,
    ) {
        let name = session.name();
        let socket = self.multiplexer.socket();
        if let Some(finding) = BusyWorktree::new(self.host.as_ref(), &socket).finding(session) {
            report.warning(finding);
        }
        if let Some(claimant) = reconciliation.claimant_of(session) {
            report.problem(format!(
                "session {name}: tmux session {} claims its worktree {}",
                claimant.tmux_name(),
                session.worktree_path().display()
            ));
        }
    }
}
