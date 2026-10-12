use std::path::{Path, PathBuf};

use super::creation_plan::CreationPlan;
use super::ports::{HostProbe, StartPoint, Workspace};
use super::service_error::ServiceError;

pub struct WorktreeRollback<'a> {
    workspace: &'a dyn Workspace,
    host: &'a dyn HostProbe,
    repo: &'a Path,
    worktree: PathBuf,
    branch: String,
    start: &'a StartPoint,
}

impl<'a> WorktreeRollback<'a> {
    pub fn new(
        workspace: &'a dyn Workspace,
        host: &'a dyn HostProbe,
        plan: &'a CreationPlan,
        start: &'a StartPoint,
    ) -> Self {
        Self {
            workspace,
            host,
            repo: plan.repo_path(),
            worktree: plan.worktree(),
            branch: plan.branch(),
            start,
        }
    }

    pub fn run(&self, error: ServiceError) -> ServiceError {
        match self.remove_created() {
            Ok(()) => error,
            Err(rollback) => error.adding_context(&format!("rollback failed: {rollback}")),
        }
    }

    fn remove_created(&self) -> Result<(), ServiceError> {
        self.remove_worktree()?;
        self.remove_branch()
    }

    fn remove_worktree(&self) -> Result<(), ServiceError> {
        let canonical = self.host.canonicalize_lenient(&self.worktree)?;
        let listed = self.workspace.list_worktrees(self.repo)?;
        if let Some(entry) = listed.iter().find(|entry| entry.path() == canonical) {
            if entry.is_locked() {
                self.workspace.unlock_worktree(self.repo, &self.worktree)?;
            }
            self.workspace.remove_worktree(self.repo, &self.worktree)?;
        }
        Ok(())
    }

    fn remove_branch(&self) -> Result<(), ServiceError> {
        if self.workspace.branch_exists(self.repo, &self.branch)? {
            self.workspace
                .delete_branch(self.repo, &self.branch, self.expected_tip())?;
        } else if self.workspace.is_branch_managed(self.repo, &self.branch)? {
            self.workspace
                .delete_branch(self.repo, &self.branch, None)?;
        }
        Ok(())
    }

    fn expected_tip(&self) -> Option<&str> {
        match self.start {
            StartPoint::Commit(sha) => Some(sha),
            StartPoint::Orphan => None,
        }
    }
}
