use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::doctor_report::DoctorReport;
use super::ports::{HostProbe, Workspace};
use super::service_ports::ServicePorts;
use crate::domain::{Config, Session};

pub struct OrphanAudit {
    workspace: Arc<dyn Workspace>,
    host: Arc<dyn HostProbe>,
}

impl OrphanAudit {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            workspace: ports.repos().workspace(),
            host: ports.repos().host(),
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
