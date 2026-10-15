use std::sync::Arc;

use crate::application::ServiceError;
use crate::application::ServicePorts;
use crate::application::ports::{HostProbe, Workspace, WorktreeEntry};
use crate::domain::Session;

pub struct WorktreeInspection {
    workspace: Arc<dyn Workspace>,
    host: Arc<dyn HostProbe>,
}

impl WorktreeInspection {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            workspace: ports.repos().workspace(),
            host: ports.repos().host(),
        }
    }

    pub fn exists(&self, session: &Session) -> bool {
        self.host.path_exists(session.worktree_path())
    }

    pub fn listed_entry(&self, session: &Session) -> Result<Option<WorktreeEntry>, ServiceError> {
        let canonical = self.host.canonicalize_lenient(session.worktree_path())?;
        let worktrees = self.workspace.list_worktrees(session.repo_path())?;
        Ok(worktrees
            .into_iter()
            .find(|entry| entry.path() == canonical))
    }

    pub fn leftover_problem(&self, session: &Session) -> Result<Option<String>, ServiceError> {
        if !self.exists(session) {
            return Ok(None);
        }
        if self.listed_entry(session)?.is_none() {
            return Ok(Some("git does not list it as a worktree".to_string()));
        }
        if !self.host.is_file(&session.worktree_path().join(".git")) {
            return Ok(Some("its .git is missing or is not a file".to_string()));
        }
        Ok(None)
    }
}
