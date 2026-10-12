use std::path::Path;
use std::sync::Arc;

use super::ports::Workspace;
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;

pub struct BranchAvailability {
    workspace: Arc<dyn Workspace>,
}

impl BranchAvailability {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            workspace: ports.repos().workspace(),
        }
    }

    pub fn refuse_taken(&self, repo: &Path, branch: &str) -> Result<(), ServiceError> {
        if self.workspace.branch_exists(repo, branch)? {
            return Err(ServiceError::Conflict(format!(
                "branch {branch} already exists"
            )));
        }
        match self.workspace.first_branch_under(repo, branch)? {
            Some(nested) => Err(ServiceError::Conflict(format!(
                "branch {branch} clashes with existing branch {nested}"
            ))),
            None => Ok(()),
        }
    }
}
