use std::path::Path;
use std::sync::Arc;

use super::doctor_report::DoctorReport;
use super::ports::{HostProbe, Multiplexer, ProcessInfo};
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
        self.audit_lost_socket(session, &self.multiplexer.socket(), report);
        if let Some(claimant) = reconciliation.claimant_of(session) {
            report.problem(format!(
                "session {name}: tmux session {} claims its worktree {}",
                claimant.tmux_name(),
                session.worktree_path().display()
            ));
        }
    }

    fn audit_lost_socket(&self, session: &Session, socket: &Path, report: &mut DoctorReport) {
        if self.host.path_exists(socket) {
            return;
        }
        let processes = self
            .host
            .processes_under_server(session.worktree_path(), socket);
        if !processes.is_empty() {
            report.warning(Self::lost_socket(session, &processes, socket));
        }
    }

    fn lost_socket(session: &Session, processes: &[ProcessInfo], socket: &Path) -> String {
        let directory = socket.parent().unwrap_or(socket).display();
        format!(
            "session {} is stopped, but processes of Ringleader's tmux server still run in its worktree {}: {}. The socket {} is gone, so it was likely removed under the running server. Recreate {directory} with mode 0700, then send SIGUSR1 to the tmux server whose command line names that socket, which makes tmux recreate it.",
            session.name(),
            session.worktree_path().display(),
            ProcessInfo::listed(processes),
            socket.display()
        )
    }
}
