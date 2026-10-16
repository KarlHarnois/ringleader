use super::creation_plan::CreationPlan;
use crate::application::ServiceError;
use crate::application::reconciliation::Reconciliation;
use crate::domain::SessionWorktree;

pub trait CheckoutStrategy {
    fn prepare(
        &self,
        plan: &CreationPlan,
        reconciliation: &Reconciliation,
    ) -> Result<SessionWorktree, ServiceError>;

    fn undo(
        &self,
        plan: &CreationPlan,
        worktree: &SessionWorktree,
        error: ServiceError,
    ) -> ServiceError;

    fn report_kept(&self, worktree: &SessionWorktree, error: ServiceError) -> ServiceError;
}
