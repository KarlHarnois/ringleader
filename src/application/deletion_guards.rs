use std::sync::Arc;

use super::branch_ownership::{BranchMutation, BranchOwnership};
use super::delete_impact::DeleteImpact;
use super::ports::{HostProbe, Multiplexer, UnmergedCommits, Workspace, WorktreeEntry};
use super::reconciliation::ManagedRow;
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use super::worktree_containment::WorktreeContainment;
use super::worktree_inspection::WorktreeInspection;
use crate::domain::{Session, SessionState};

pub struct DeletionGuards {
    inspection: WorktreeInspection,
    ownership: BranchOwnership,
    workspace: Arc<dyn Workspace>,
    multiplexer: Arc<dyn Multiplexer>,
    host: Arc<dyn HostProbe>,
}

impl DeletionGuards {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            inspection: WorktreeInspection::new(ports),
            ownership: BranchOwnership::new(ports),
            workspace: ports.repos().workspace(),
            multiplexer: ports.tmux().multiplexer(),
            host: ports.repos().host(),
        }
    }

    pub fn measure(&self, row: &ManagedRow) -> Result<Measurement, ServiceError> {
        let session = row.session();
        self.refuse_unmeasurable(session)?;
        self.ownership
            .refuse_unmarked(session, BranchMutation::Delete)?;
        let dirty = self.workspace.dirty_state(session.worktree_path())?;
        let commits = self.workspace.unmerged_commits(
            session.repo_path(),
            session.branch(),
            session.worktree_path(),
        )?;
        let pane_sessions = self.pane_sessions(row)?;
        let processes = self
            .host
            .processes_in(session.worktree_path(), &pane_sessions);
        let branch = self.branch_presence(session, &commits)?;
        let impact = DeleteImpact::measured(&dirty, &commits, processes)
            .while_working(row.state() == SessionState::Working);
        Ok(Measurement {
            impact,
            commits,
            branch,
        })
    }

    pub fn check_removable(&self, session: &Session) -> Result<(), ServiceError> {
        self.refuse_outside_worktree_root(session)?;
        self.refuse_what_git_refuses(session)
    }

    fn refuse_unmeasurable(&self, session: &Session) -> Result<(), ServiceError> {
        let reason = match self.workspace.validate_repo(session.repo_path()) {
            Err(error) => Some(error.to_string()),
            Ok(_) => self.inspection.leftover_problem(session)?,
        };
        match reason {
            None => Ok(()),
            Some(reason) => Err(ServiceError::Unmeasurable(format!(
                "git cannot measure the worktree {} of repository {}: {reason}. Restore the repository, remove a leftover worktree directory by hand, or delete with --forget",
                session.worktree_path().display(),
                session.repo_path().display()
            ))),
        }
    }

    fn pane_sessions(&self, row: &ManagedRow) -> Result<Vec<u32>, ServiceError> {
        if !row.is_live() {
            return Ok(Vec::new());
        }
        Ok(self.multiplexer.pane_pids(row.session().tmux_name())?)
    }

    fn branch_presence(
        &self,
        session: &Session,
        commits: &UnmergedCommits,
    ) -> Result<BranchPresence, ServiceError> {
        if commits.tip().is_some() {
            return Ok(BranchPresence::Present);
        }
        if self
            .workspace
            .branch_exists(session.repo_path(), session.branch())?
        {
            return Ok(BranchPresence::Unborn);
        }
        Ok(BranchPresence::Missing)
    }

    fn refuse_outside_worktree_root(&self, session: &Session) -> Result<(), ServiceError> {
        let root = session.naming().worktree_root();
        let containment = WorktreeContainment::new(self.host.as_ref());
        if !containment.contains(root, session.worktree_path()) {
            return Err(ServiceError::Conflict(format!(
                "worktree {} is not inside the worktree root {}, refusing to delete it",
                session.worktree_path().display(),
                root.display()
            )));
        }
        Ok(())
    }

    fn refuse_what_git_refuses(&self, session: &Session) -> Result<(), ServiceError> {
        let canonical = self.host.canonicalize_lenient(session.worktree_path())?;
        let worktrees = self.workspace.list_worktrees(session.repo_path())?;
        let own = worktrees.iter().find(|entry| entry.path() == canonical);
        if let Some(reason) = own.and_then(Self::removal_refusal) {
            return Err(ServiceError::Conflict(format!(
                "git refuses to remove {}: {reason}",
                session.worktree_path().display()
            )));
        }
        let elsewhere = worktrees
            .iter()
            .find(|entry| entry.path() != canonical && entry.branch() == Some(session.branch()));
        match elsewhere {
            Some(entry) => Err(ServiceError::Conflict(format!(
                "branch {} is checked out in another worktree, {}",
                session.branch(),
                entry.path().display()
            ))),
            None => Ok(()),
        }
    }

    fn removal_refusal(entry: &WorktreeEntry) -> Option<&'static str> {
        if entry.is_main() {
            Some("it is the main worktree")
        } else if entry.is_locked() {
            Some("it is locked, run git worktree unlock first")
        } else {
            None
        }
    }
}

pub struct Measurement {
    impact: DeleteImpact,
    commits: UnmergedCommits,
    branch: BranchPresence,
}

impl Measurement {
    pub fn impact(&self) -> &DeleteImpact {
        &self.impact
    }

    pub fn into_impact(self) -> DeleteImpact {
        self.impact
    }

    pub fn commits(&self) -> &UnmergedCommits {
        &self.commits
    }

    pub fn branch(&self) -> BranchPresence {
        self.branch
    }

    pub fn log_recovery_point(&self, session: &Session) {
        log::info!(
            "deleting session {}: branch {} tip {}, worktree HEAD {}",
            session.name(),
            session.branch(),
            self.commits.tip_label(),
            self.commits.head_label()
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchPresence {
    Present,
    Unborn,
    Missing,
}
