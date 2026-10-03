use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::doctor_report::DoctorReport;
use super::ports::{HostProbe, Multiplexer, Workspace};
use super::reconciliation::Reconciliation;
use super::service_ports::ServicePorts;
use super::worktree_inspection::WorktreeInspection;
use crate::domain::{Config, Session, SessionState};

pub struct RegistryAudit {
    inspection: WorktreeInspection,
    workspace: Arc<dyn Workspace>,
    multiplexer: Arc<dyn Multiplexer>,
    host: Arc<dyn HostProbe>,
}

impl RegistryAudit {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            inspection: WorktreeInspection::new(ports),
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
            self.report_orphans(reconciliation.entries(), config, report);
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
            Ok(true) => {}
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
                "session {name} was created under worktree_root {}, branch_prefix {}, and tmux_prefix {}, which differ from the config; Ctrl-Q does not return from a session whose name lacks the current tmux_prefix",
                recorded.worktree_root().display(),
                recorded.branch_prefix(),
                recorded.tmux_prefix()
            ));
        }
    }

    fn report_orphans(&self, entries: &[Session], config: &Config, report: &mut DoctorReport) {
        let canonical = |path: &Path| {
            self.host
                .canonicalize_lenient(path)
                .unwrap_or_else(|_| path.to_path_buf())
        };
        let claimed: BTreeSet<PathBuf> = entries
            .iter()
            .map(|entry| canonical(entry.worktree_path()))
            .collect();
        let root = config.naming().worktree_root();
        for directory in self.workspace.list_worktree_dirs(root).unwrap_or_default() {
            if !claimed.contains(&canonical(&directory)) {
                report.warning(format!(
                    "orphaned worktree {}, no session claims it",
                    directory.display()
                ));
            }
        }
        let repos: BTreeSet<PathBuf> = entries
            .iter()
            .map(|entry| entry.repo_path().to_path_buf())
            .chain(config.repos().iter().map(|repo| repo.path().to_path_buf()))
            .collect();
        for repo in repos {
            self.report_orphan_branches(&repo, config.naming().branch_prefix(), report);
        }
    }

    fn report_orphan_branches(&self, repo: &Path, prefix: &str, report: &mut DoctorReport) {
        let (Ok(branches), Ok(worktrees)) = (
            self.workspace.list_branches(repo, prefix),
            self.workspace.list_worktrees(repo),
        ) else {
            return;
        };
        for branch in branches {
            if worktrees
                .iter()
                .all(|entry| entry.branch() != Some(branch.as_str()))
            {
                report.warning(format!(
                    "orphaned branch {branch} in {}, no worktree has it",
                    repo.display()
                ));
            }
        }
    }
}
