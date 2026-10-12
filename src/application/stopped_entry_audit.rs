use std::path::Path;
use std::sync::Arc;

use super::doctor_report::DoctorReport;
use super::ports::{HostProbe, Multiplexer, ProcessInfo};
use super::reconciliation::Reconciliation;
use super::service_ports::ServicePorts;
use crate::domain::{EarlierServer, Session};

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
        let processes = self.host.processes_in(session.worktree_path(), &[]);
        let private_socket = self.multiplexer.socket();
        match EarlierServer::of(session, &private_socket) {
            Some(earlier) => {
                report.warning(earlier.audit(name.as_str(), &Self::worktree_processes(&processes)))
            }
            None if !processes.is_empty() => {
                report.warning(Self::lost_socket(session, &processes, &private_socket))
            }
            None => {}
        }
        if let Some(claimant) = reconciliation.claimant_of(session) {
            report.problem(format!(
                "session {name}: tmux session {} claims its worktree {}",
                claimant.tmux_name(),
                session.worktree_path().display()
            ));
        }
    }

    fn worktree_processes(processes: &[ProcessInfo]) -> String {
        if processes.is_empty() {
            return "no process runs in its worktree".to_string();
        }
        format!(
            "processes still run in its worktree: {}",
            ProcessInfo::listed(processes)
        )
    }

    fn lost_socket(session: &Session, processes: &[ProcessInfo], socket: &Path) -> String {
        let directory = socket.parent().unwrap_or(socket).display();
        format!(
            "session {} is stopped, but processes still run in its worktree {}: {}. The socket {} of Ringleader's tmux server may have been removed under the running server. Recreate {directory} with mode 0700, then send SIGUSR1 to the tmux server whose command line names that socket, which makes tmux recreate it.",
            session.name(),
            session.worktree_path().display(),
            ProcessInfo::listed(processes),
            socket.display()
        )
    }
}
