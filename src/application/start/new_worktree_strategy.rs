use std::sync::Arc;

use super::checkout_strategy::CheckoutStrategy;
use super::creation_plan::CreationPlan;
use super::start_point_resolver::StartPointResolver;
use super::worktree_rollback::WorktreeRollback;
use crate::application::ServiceError;
use crate::application::ServicePorts;
use crate::application::checks::branch_availability::BranchAvailability;
use crate::application::ports::{HostProbe, StartPoint, Workspace};
use crate::application::reconciliation::Reconciliation;
use crate::application::repo_merge::RepoMerge;
use crate::domain::SessionWorktree;

pub struct NewWorktreeStrategy {
    branches: BranchAvailability,
    workspace: Arc<dyn Workspace>,
    host: Arc<dyn HostProbe>,
}

impl NewWorktreeStrategy {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            branches: BranchAvailability::new(ports),
            workspace: ports.repos().workspace(),
            host: ports.repos().host(),
        }
    }

    fn check_worktree_available(
        &self,
        plan: &CreationPlan,
        reconciliation: &Reconciliation,
    ) -> Result<(), ServiceError> {
        let claimed_by_entry = reconciliation
            .entries()
            .iter()
            .any(|entry| entry.worktree_path() == plan.worktree());
        let canonical = self.host.canonicalize_lenient(&plan.worktree())?;
        let listed_by_git = self
            .workspace
            .list_worktrees(plan.repo_path())?
            .iter()
            .any(|entry| entry.path() == canonical);
        if claimed_by_entry || listed_by_git || self.host.path_exists(&plan.worktree()) {
            return Err(ServiceError::Conflict(format!(
                "worktree {} already exists",
                plan.worktree().display()
            )));
        }
        Ok(())
    }

    fn resolve_start(
        &self,
        plan: &CreationPlan,
        reconciliation: &Reconciliation,
    ) -> Result<StartPoint, ServiceError> {
        let session_branches = RepoMerge::new(self.host.as_ref())
            .session_branches_in(plan.repo_path(), reconciliation.entries());
        StartPointResolver::new(self.workspace.as_ref())
            .skipping(session_branches)
            .resolve(plan.repo_path(), plan.base_branch())
    }

    fn create_marked_worktree(
        &self,
        plan: &CreationPlan,
        start: &StartPoint,
    ) -> Result<(), ServiceError> {
        let branch = plan.branch();
        self.workspace
            .create_worktree(plan.repo_path(), &plan.worktree(), &branch, start)
            .and_then(|()| {
                self.workspace
                    .mark_branch_managed(plan.repo_path(), &branch)
            })
            .map_err(|error| self.rollback(plan, start).run(error.into()))
    }

    fn rollback<'a>(
        &'a self,
        plan: &'a CreationPlan,
        start: &'a StartPoint,
    ) -> WorktreeRollback<'a> {
        WorktreeRollback::new(self.workspace.as_ref(), self.host.as_ref(), plan, start)
    }
}

impl CheckoutStrategy for NewWorktreeStrategy {
    fn prepare(
        &self,
        plan: &CreationPlan,
        reconciliation: &Reconciliation,
    ) -> Result<SessionWorktree, ServiceError> {
        self.check_worktree_available(plan, reconciliation)?;
        self.branches
            .refuse_taken(plan.repo_path(), &plan.branch())?;
        let start = self.resolve_start(plan, reconciliation)?;
        self.create_marked_worktree(plan, &start)?;
        Ok(plan.new_worktree_from(&start))
    }

    fn undo(
        &self,
        plan: &CreationPlan,
        worktree: &SessionWorktree,
        error: ServiceError,
    ) -> ServiceError {
        let start = StartPoint::from_base_commit(worktree.base_commit());
        self.rollback(plan, &start).run(error)
    }

    fn report_kept(&self, worktree: &SessionWorktree, error: ServiceError) -> ServiceError {
        error.adding_context(&format!(
            "kept the worktree {} and the branch {}, run ringleader doctor",
            worktree.path().display(),
            worktree.branch()
        ))
    }
}
