use std::path::PathBuf;

use super::ports::{HostProbe, Workspace};
use super::service_error::ServiceError;
use crate::domain::Session;

pub struct BranchRenaming<'a> {
    workspace: &'a dyn Workspace,
    location: PathBuf,
    old: &'a str,
    new: &'a str,
}

impl<'a> BranchRenaming<'a> {
    pub fn new(
        workspace: &'a dyn Workspace,
        host: &dyn HostProbe,
        session: &'a Session,
        renamed: &'a Session,
    ) -> Self {
        let worktree = session.worktree_path();
        let location = if host.path_exists(worktree) {
            worktree
        } else {
            session.repo_path()
        };
        Self {
            workspace,
            location: location.to_path_buf(),
            old: session.branch(),
            new: renamed.branch(),
        }
    }

    pub fn changes_branch(&self) -> bool {
        self.old != self.new
    }

    pub fn apply(&self) -> Result<(), ServiceError> {
        if !self.changes_branch() {
            return Ok(());
        }
        Ok(self
            .workspace
            .rename_branch(&self.location, self.old, self.new)?)
    }

    pub fn drop_stale_upstream(&self) {
        if !self.changes_branch() {
            return;
        }
        if let Err(error) = self
            .workspace
            .drop_stale_upstream(&self.location, self.old, self.new)
        {
            log::warn!(
                "renamed branch {} to {} but kept its upstream on {}: {error}",
                self.old,
                self.new,
                self.old
            );
        }
    }

    pub fn roll_back(&self, error: ServiceError) -> ServiceError {
        if !self.changes_branch() {
            return error;
        }
        match self
            .workspace
            .rename_branch(&self.location, self.new, self.old)
        {
            Ok(()) => error,
            Err(rollback) => error.adding_context(&format!(
                "renaming the branch back also failed ({rollback}), git now holds {}",
                self.new
            )),
        }
    }
}
