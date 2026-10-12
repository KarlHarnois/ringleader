use std::path::Path;
use std::sync::Arc;

use super::creation_plan::CreateRequest;
use super::delete_impact::{DeleteConfirmation, DeleteImpact};
use super::delete_outcome::DeleteOutcome;
use super::ports::Multiplexer;
use super::reconciliation::{ManagedRow, Reconciliation};
use super::rename_request::RenameRequest;
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use super::session_creation::SessionCreation;
use super::session_deletion::SessionDeletion;
use super::session_renaming::SessionRenaming;
use super::session_restart::SessionRestart;
use crate::domain::{Config, Session, SessionState};

pub struct SessionLifecycle {
    multiplexer: Arc<dyn Multiplexer>,
    creation: SessionCreation,
    renaming: SessionRenaming,
    deletion: SessionDeletion,
    restart: SessionRestart,
}

impl SessionLifecycle {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            multiplexer: ports.tmux().multiplexer(),
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
        self.creation
            .create(request, reconciliation, config, config_path)
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
        let row = self.managed_target(reconciliation, name)?;
        self.restart
            .restart(&row, reconciliation, config, config_path)
    }

    pub fn rename(
        &self,
        reconciliation: &Reconciliation,
        request: &RenameRequest,
        config: &Config,
    ) -> Result<Session, ServiceError> {
        let row = self.managed_target(reconciliation, request.old_name())?;
        self.renaming.rename(&row, request, reconciliation, config)
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
        Ok(row)
    }

    fn managed_target<'r>(
        &self,
        reconciliation: &'r Reconciliation,
        name: &str,
    ) -> Result<ManagedRow<'r>, ServiceError> {
        let row = reconciliation.managed(name)?;
        if row.state() == SessionState::Stopped {
            self.refuse_if_live_elsewhere(row.session(), reconciliation)?;
        }
        Ok(row)
    }

    fn refuse_if_live_elsewhere(
        &self,
        session: &Session,
        reconciliation: &Reconciliation,
    ) -> Result<(), ServiceError> {
        let socket = session.socket_path();
        if self
            .multiplexer
            .has_session_on(socket, session.tmux_name(), session.worktree_path())?
        {
            return Err(ServiceError::Conflict(format!(
                "session {} is still live on the tmux server at {}",
                session.name(),
                socket.display()
            )));
        }
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
