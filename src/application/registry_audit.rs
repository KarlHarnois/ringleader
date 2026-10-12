use std::sync::Arc;

use super::branch_ownership::BranchOwnership;
use super::doctor_report::DoctorReport;
use super::orphan_audit::OrphanAudit;
use super::ports::{Multiplexer, Workspace};
use super::reconciliation::Reconciliation;
use super::service_ports::ServicePorts;
use super::worktree_inspection::WorktreeInspection;
use crate::domain::{Config, Session, SessionState};

pub struct RegistryAudit {
    inspection: WorktreeInspection,
    orphans: OrphanAudit,
    workspace: Arc<dyn Workspace>,
    multiplexer: Arc<dyn Multiplexer>,
}

impl RegistryAudit {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            inspection: WorktreeInspection::new(ports),
            orphans: OrphanAudit::new(ports),
            workspace: ports.repos().workspace(),
            multiplexer: ports.tmux().multiplexer(),
        }
    }

    pub fn audit(
        &self,
        reconciliation: &Reconciliation,
        config: Option<&Config>,
        report: &mut DoctorReport,
    ) {
        for row in reconciliation.rows() {
            if let Some(advice) = row
                .unmanaged_session()
                .and_then(|unmanaged| unmanaged.advice())
            {
                report.problem(format!("session {}: {advice}", row.name()));
            }
        }
        for (session, state) in reconciliation.managed_entries() {
            self.audit_entry(session, state, reconciliation, report);
            if let Some(config) = config {
                Self::audit_against_config(session, config, report);
            }
        }
        if let Some(config) = config {
            self.orphans.audit(reconciliation.entries(), config, report);
        }
    }

    fn audit_entry(
        &self,
        session: &Session,
        state: SessionState,
        reconciliation: &Reconciliation,
        report: &mut DoctorReport,
    ) {
        let name = session.name();
        if !self.inspection.exists(session) {
            report.problem(format!(
                "session {name}: worktree {} is missing",
                session.worktree_path().display()
            ));
        }
        match self
            .workspace
            .branch_exists(session.repo_path(), session.branch())
        {
            Ok(true) => self.audit_existing_branch(session, report),
            Ok(false) => report.problem(format!(
                "session {name}: branch {} is missing",
                session.branch()
            )),
            Err(error) => {
                report.problem(format!("session {name}: cannot read its branch: {error}"))
            }
        }
        if let Ok(Some(problem)) = self.inspection.leftover_problem(session) {
            report.problem(format!(
                "session {name}: worktree {} {problem}, remove the directory by hand or run ringleader delete {name} --forget",
                session.worktree_path().display()
            ));
        }
        if state == SessionState::Stopped {
            self.audit_stopped(session, reconciliation, report);
        }
    }

    fn audit_existing_branch(&self, session: &Session, report: &mut DoctorReport) {
        match self
            .workspace
            .is_branch_managed(session.repo_path(), session.branch())
        {
            Ok(true) => self.audit_branch_name(session, report),
            Ok(false) => report.warning(format!(
                "session {}: branch {} has no Ringleader marker, so delete and rename refuse it; {}",
                session.name(),
                session.branch(),
                BranchOwnership::marking_advice(session)
            )),
            Err(_) => {}
        }
    }

    fn audit_branch_name(&self, session: &Session, report: &mut DoctorReport) {
        if session.has_branch_named_after_it() {
            return;
        }
        if let Ok(false) = self
            .workspace
            .is_branch_published(session.repo_path(), session.branch())
        {
            report.warning(format!(
                "session {}: branch {} differs from the session name, rename the session to bring them in line",
                session.name(),
                session.branch()
            ));
        }
    }

    fn audit_stopped(
        &self,
        session: &Session,
        reconciliation: &Reconciliation,
        report: &mut DoctorReport,
    ) {
        let name = session.name();
        let socket = session.socket_path();
        match self
            .multiplexer
            .has_session_on(socket, session.tmux_name(), session.worktree_path())
        {
            Ok(true) => report.problem(format!(
                "session {name} is live on the tmux server at {}",
                socket.display()
            )),
            Ok(false) => {}
            Err(error) => report.problem(format!(
                "session {name}: cannot check {}: {error}",
                socket.display()
            )),
        }
        if let Some(claimant) = reconciliation.claimant_of(session) {
            report.problem(format!(
                "session {name}: tmux session {} claims its worktree {}",
                claimant.tmux_name(),
                session.worktree_path().display()
            ));
        }
    }

    fn audit_against_config(session: &Session, config: &Config, report: &mut DoctorReport) {
        let name = session.name();
        if config.agent(session.agent()).is_none() {
            report.problem(format!(
                "session {name}: agent {} is no longer configured",
                session.agent()
            ));
        }
        let recorded = session.naming();
        let current = config.naming();
        if recorded != current {
            report.warning(format!(
                "session {name} was created under worktree_root {} and tmux_prefix {}, which differ from the config; Ctrl-Q does not return from a session whose name lacks the current tmux_prefix",
                recorded.worktree_root().display(),
                recorded.tmux_prefix()
            ));
        }
    }
}
