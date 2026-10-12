use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::creation_plan::CreateRequest;
use super::delete_impact::{DeleteConfirmation, DeleteImpact};
use super::delete_outcome::DeleteOutcome;
use super::ports::{HostProbe, Multiplexer, ProcessInfo};
use super::reconciliation::{ManagedRow, Reconciliation};
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use super::session_creation::SessionCreation;
use super::session_deletion::SessionDeletion;
use super::session_renaming::SessionRenaming;
use super::session_restart::SessionRestart;
use super::worktree_containment::WorktreeContainment;
use crate::domain::{Config, EarlierServer, ReturnKey, Session, SessionState};

pub struct SessionLifecycle {
    multiplexer: Arc<dyn Multiplexer>,
    host: Arc<dyn HostProbe>,
    launch_directory: PathBuf,
    creation: SessionCreation,
    renaming: SessionRenaming,
    deletion: SessionDeletion,
    restart: SessionRestart,
}

impl SessionLifecycle {
    pub fn new(ports: &ServicePorts, launch_directory: &Path) -> Self {
        Self {
            multiplexer: ports.tmux().multiplexer(),
            host: ports.repos().host(),
            launch_directory: launch_directory.to_path_buf(),
            creation: SessionCreation::new(ports),
            renaming: SessionRenaming::new(ports),
            deletion: SessionDeletion::new(ports),
            restart: SessionRestart::new(ports),
        }
    }

    pub fn create(
        &self,
        request: &CreateRequest,
        reconciliation: &Reconciliation,
        config: &Config,
        config_path: &Path,
    ) -> Result<Session, ServiceError> {
        self.sync_server_config(config.return_key())?;
        self.creation
            .create(request, reconciliation, config, config_path)
    }

    pub fn sync_server_config(&self, return_key: &ReturnKey) -> Result<(), ServiceError> {
        match self.multiplexer.server_config(return_key)? {
            Some(running) if !running.is_current() => {
                Ok(self.multiplexer.apply_config(return_key)?)
            }
            _ => Ok(()),
        }
    }

    pub fn delete_impact(
        &self,
        reconciliation: &Reconciliation,
        name: &str,
    ) -> Result<DeleteImpact, ServiceError> {
        let row = self.deletable_target(reconciliation, name)?;
        self.deletion.impact(&row)
    }

    pub fn delete(
        &self,
        reconciliation: &Reconciliation,
        name: &str,
        confirmation: &DeleteConfirmation,
    ) -> Result<DeleteOutcome, ServiceError> {
        let row = self.deletable_target(reconciliation, name)?;
        let remaining = reconciliation.entries_without(name);
        self.deletion.delete(&row, &remaining, confirmation)
    }

    pub fn restart(
        &self,
        reconciliation: &Reconciliation,
        name: &str,
        config: &Config,
        config_path: &Path,
    ) -> Result<Session, ServiceError> {
        let row = self.quiet_target(reconciliation, name)?;
        self.sync_server_config(config.return_key())?;
        self.restart
            .restart(&row, reconciliation, config, config_path)
    }

    pub fn rename(
        &self,
        reconciliation: &Reconciliation,
        old: &str,
        new: &str,
        keep_branch: bool,
    ) -> Result<Session, ServiceError> {
        let row = self.quiet_target(reconciliation, old)?;
        self.renaming.rename(&row, new, keep_branch, reconciliation)
    }

    fn deletable_target<'r>(
        &self,
        reconciliation: &'r Reconciliation,
        name: &str,
    ) -> Result<ManagedRow<'r>, ServiceError> {
        let row = self.managed_target(reconciliation, name)?;
        if self.multiplexer.caller_session()?.as_deref() == Some(row.session().tmux_name()) {
            return Err(ServiceError::Conflict(format!(
                "session {name} runs this Ringleader, delete it from another terminal"
            )));
        }
        self.refuse_launch_inside(row.session())?;
        Ok(row)
    }

    fn refuse_launch_inside(&self, session: &Session) -> Result<(), ServiceError> {
        let containment = WorktreeContainment::new(self.host.as_ref());
        if !containment.holds(session.worktree_path(), &self.launch_directory) {
            return Ok(());
        }
        Err(ServiceError::Conflict(format!(
            "Ringleader runs from {}, inside the worktree of session {}, delete it from another directory",
            self.launch_directory.display(),
            session.name()
        )))
    }

    fn quiet_target<'r>(
        &self,
        reconciliation: &'r Reconciliation,
        name: &str,
    ) -> Result<ManagedRow<'r>, ServiceError> {
        let row = self.managed_target(reconciliation, name)?;
        if row.state() == SessionState::Stopped {
            self.refuse_busy_worktree(row.session())?;
        }
        Ok(row)
    }

    fn managed_target<'r>(
        &self,
        reconciliation: &'r Reconciliation,
        name: &str,
    ) -> Result<ManagedRow<'r>, ServiceError> {
        let row = reconciliation.managed(name)?;
        if row.state() == SessionState::Stopped {
            self.refuse_if_claimed(row.session(), reconciliation)?;
        }
        Ok(row)
    }

    fn refuse_busy_worktree(&self, session: &Session) -> Result<(), ServiceError> {
        let processes = self.host.processes_in(session.worktree_path(), &[]);
        if processes.is_empty() {
            return Ok(());
        }
        let busy = format!(
            "session {} is stopped, but processes still run in its worktree {}: {}. End them first.",
            session.name(),
            session.worktree_path().display(),
            Self::listed(&processes)
        );
        let earlier = EarlierServer::of(session, &self.multiplexer.socket());
        Err(ServiceError::Conflict(match earlier {
            Some(earlier) => format!("{busy} {}", earlier.refusal(session.name().as_str())),
            None => busy,
        }))
    }

    fn listed(processes: &[ProcessInfo]) -> String {
        let listed: Vec<String> = processes.iter().map(ToString::to_string).collect();
        listed.join(", ")
    }

    fn refuse_if_claimed(
        &self,
        session: &Session,
        reconciliation: &Reconciliation,
    ) -> Result<(), ServiceError> {
        match reconciliation.claimant_of(session) {
            Some(claimant) => Err(ServiceError::Conflict(format!(
                "tmux session {} claims the worktree {} of session {}",
                claimant.tmux_name(),
                session.worktree_path().display(),
                session.name()
            ))),
            None => Ok(()),
        }
    }
}
