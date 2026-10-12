use std::sync::Arc;

use super::ports::{BranchMarker, Workspace};
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use crate::domain::Session;

pub struct BranchOwnership {
    workspace: Arc<dyn Workspace>,
}

impl BranchOwnership {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            workspace: ports.repos().workspace(),
        }
    }

    pub fn refuse_unmarked(
        &self,
        session: &Session,
        mutation: BranchMutation,
    ) -> Result<(), ServiceError> {
        if self.is_marked_or_gone(session)? {
            return Ok(());
        }
        Err(ServiceError::Conflict(format!(
            "branch {} has no Ringleader marker, refusing to {} it; {}",
            session.branch(),
            mutation.verb(),
            mutation.advice(session)
        )))
    }

    pub fn marking_advice(session: &Session) -> String {
        let command = BranchMarker::new(session.branch()).marking_command(session.repo_path());
        format!("if Ringleader created it, run {command}")
    }

    fn is_marked_or_gone(&self, session: &Session) -> Result<bool, ServiceError> {
        let repo = session.repo_path();
        let branch = session.branch();
        Ok(self.workspace.is_branch_managed(repo, branch)?
            || !self.workspace.branch_exists(repo, branch)?)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchMutation {
    Delete,
    Rename,
}

impl BranchMutation {
    fn verb(self) -> &'static str {
        match self {
            BranchMutation::Delete => "delete",
            BranchMutation::Rename => "rename",
        }
    }

    fn advice(self, session: &Session) -> String {
        let marking = BranchOwnership::marking_advice(session);
        match self {
            BranchMutation::Delete => {
                format!("{marking}, or delete with --forget to drop only the session")
            }
            BranchMutation::Rename => {
                format!(
                    "{marking}, otherwise the session keeps its name, since rename moves the branch with it"
                )
            }
        }
    }
}
