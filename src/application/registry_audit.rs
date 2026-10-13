use std::sync::Arc;

use super::branch_ownership::BranchOwnership;
use super::busy_worktree::BusyWorktree;
use super::doctor_report::DoctorReport;
use super::orphan_audit::OrphanAudit;
use super::ports::{HostProbe, Multiplexer, Workspace};
use super::reconciliation::Reconciliation;
use super::service_ports::ServicePorts;
use super::worktree_inspection::WorktreeInspection;
use crate::domain::{Config, Session, SessionState};

pub struct RegistryAudit {
    inspection: WorktreeInspection,
    orphans: OrphanAudit,
    workspace: Arc<dyn Workspace>,
    multiplexer: Arc<dyn Multiplexer>,
    host: Arc<dyn HostProbe>,
}

impl RegistryAudit {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            inspection: WorktreeInspection::new(ports),
            orphans: OrphanAudit::new(ports),
            workspace: ports.repos().workspace(),
            multiplexer: ports.tmux().multiplexer(),
            host: ports.repos().host(),
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
            Ok(true) => self.audit_marker(session, report),
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

    fn audit_stopped(
        &self,
        session: &Session,
        reconciliation: &Reconciliation,
        report: &mut DoctorReport,
    ) {
        let socket = self.multiplexer.socket();
        if let Some(finding) = BusyWorktree::new(self.host.as_ref(), &socket).finding(session) {
            report.warning(finding);
        }
        if let Some(claimant) = reconciliation.claimant_of(session) {
            report.problem(format!(
                "session {}: tmux session {} claims its worktree {}",
                session.name(),
                claimant.tmux_name(),
                session.worktree_path().display()
            ));
        }
    }

    fn audit_marker(&self, session: &Session, report: &mut DoctorReport) {
        if let Ok(false) = self
            .workspace
            .is_branch_managed(session.repo_path(), session.branch())
        {
            report.warning(format!(
                "session {}: branch {} has no Ringleader marker, so delete and rename refuse it; {}",
                session.name(),
                session.branch(),
                BranchOwnership::marking_advice(session)
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
        if recorded.worktree_root() != current.worktree_root() {
            report.warning(format!(
                "session {name} was created under worktree_root {}, which differs from the config",
                recorded.worktree_root().display()
            ));
        }
    }
}
