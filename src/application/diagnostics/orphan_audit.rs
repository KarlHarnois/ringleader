use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::doctor_report::DoctorReport;
use crate::application::ServicePorts;
use crate::application::ports::{HostProbe, ScratchSpace, SessionRegistry, Workspace};
use crate::application::reconciliation::Reconciliation;
use crate::application::scratch_claims::ScratchClaims;
use crate::domain::{Config, Session};

pub struct OrphanAudit {
    workspace: Arc<dyn Workspace>,
    host: Arc<dyn HostProbe>,
    scratch: Arc<dyn ScratchSpace>,
    registry: Arc<dyn SessionRegistry>,
}

impl OrphanAudit {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            workspace: ports.repos().workspace(),
            host: ports.repos().host(),
            scratch: ports.state().scratch_space(),
            registry: ports.state().registry(),
        }
    }

    pub fn audit_scratch(&self, reconciliation: &Reconciliation, report: &mut DoctorReport) {
        self.report_moved_aside_registries(report);
        let Ok(directories) = self.scratch.directories() else {
            return;
        };
        let claims = ScratchClaims::new(
            self.scratch.as_ref(),
            reconciliation.entries(),
            reconciliation.observations(),
        );
        for orphan in claims.orphans(directories) {
            report.warning(format!(
                "orphaned scratch directory {}, no session claims it",
                orphan.display()
            ));
        }
    }

    fn report_moved_aside_registries(&self, report: &mut DoctorReport) {
        for copy in self.registry.moved_aside_copies().unwrap_or_default() {
            report.warning(format!(
                "corrupt registry {} keeps other sessions' orphaned scratch directories until it is restored or removed",
                copy.display()
            ));
        }
    }

    pub fn audit(&self, entries: &[Session], config: &Config, report: &mut DoctorReport) {
        self.report_orphan_worktrees(entries, config, report);
        for repo in Self::repos(entries, config) {
            self.report_orphan_branches(&repo, report);
        }
    }

    fn report_orphan_worktrees(
        &self,
        entries: &[Session],
        config: &Config,
        report: &mut DoctorReport,
    ) {
        let claimed: BTreeSet<PathBuf> = entries
            .iter()
            .map(|entry| self.host.canonical_or_given(entry.worktree_path()))
            .collect();
        let root = config.naming().worktree_root();
        for directory in self.workspace.list_worktree_dirs(root).unwrap_or_default() {
            if !claimed.contains(&self.host.canonical_or_given(&directory)) {
                report.warning(format!(
                    "orphaned worktree {}, no session claims it",
                    directory.display()
                ));
            }
        }
    }

    fn repos(entries: &[Session], config: &Config) -> BTreeSet<PathBuf> {
        entries
            .iter()
            .map(|entry| entry.repo_path().to_path_buf())
            .chain(config.repos().iter().map(|repo| repo.path().to_path_buf()))
            .collect()
    }

    fn report_orphan_branches(&self, repo: &Path, report: &mut DoctorReport) {
        let (Ok(branches), Ok(worktrees)) = (
            self.workspace.list_managed_branches(repo),
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
