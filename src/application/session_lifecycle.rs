use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::agent_hooks::AgentHooks;
use super::ports::{HostProbe, Multiplexer};
use super::reconciliation::{ManagedRow, Reconciliation};
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use crate::application::CreateRequest;
use crate::application::DeleteOutcome;
use crate::application::StartOutcome;
use crate::application::checks::busy_worktree::BusyWorktree;
use crate::application::checks::worktree_containment::WorktreeContainment;
use crate::application::deletion::session_deletion::SessionDeletion;
use crate::application::merge::merge_report::MergeReport;
use crate::application::merge::session_merge::SessionMerge;
use crate::application::renaming::rename_request::RenameRequest;
use crate::application::renaming::session_renaming::SessionRenaming;
use crate::application::session_stop::SessionStop;
use crate::application::start::session_creation::SessionCreation;
use crate::application::start::session_restart::SessionRestart;
use crate::application::{DeleteConfirmation, DeleteImpact};
use crate::domain::{Config, KeyBindings, Session, SessionState};

pub struct SessionLifecycle {
    multiplexer: Arc<dyn Multiplexer>,
    host: Arc<dyn HostProbe>,
    launch_directory: PathBuf,
    creation: SessionCreation,
    renaming: SessionRenaming,
    deletion: SessionDeletion,
    restart: SessionRestart,
    stopping: SessionStop,
    merging: SessionMerge,
    hooks: Arc<AgentHooks>,
}

impl SessionLifecycle {
    const RESTORED_SOCKET: &'static str =
        "recreated the socket of Ringleader's tmux server, which was removed while the server ran";

    pub fn new(ports: &ServicePorts, launch_directory: &Path) -> Self {
        let hooks = Arc::new(AgentHooks::new(ports.tmux().multiplexer()));
        Self {
            multiplexer: ports.tmux().multiplexer(),
            host: ports.repos().host(),
            launch_directory: launch_directory.to_path_buf(),
            creation: SessionCreation::new(ports, Arc::clone(&hooks)),
            renaming: SessionRenaming::new(ports, Arc::clone(&hooks)),
            deletion: SessionDeletion::new(ports),
            restart: SessionRestart::new(ports, Arc::clone(&hooks)),
            stopping: SessionStop::new(ports),
            merging: SessionMerge::new(ports),
            hooks,
        }
    }

    pub fn poll_startup_input(&self) {
        self.hooks.poll_startup_input();
    }

    pub fn startup_wait_note(&self) -> Option<&'static str> {
        self.hooks.startup_wait_note()
    }

    pub fn wait_startup_input(&self, stop: impl Fn() -> bool) {
        self.hooks.wait_startup_input(stop);
    }

    pub fn create(
        &self,
        request: &CreateRequest,
        reconciliation: &Reconciliation,
        config: &Config,
        config_path: &Path,
    ) -> Result<StartOutcome, ServiceError> {
        self.sync_server_config(config.key_bindings())?;
        self.creation
            .create(request, reconciliation, config, config_path)
    }

    pub fn restore_server_socket(&self) -> Result<Option<String>, ServiceError> {
        let restored = self.multiplexer.restore_socket()?;
        Ok(restored.then(|| Self::RESTORED_SOCKET.to_string()))
    }

    pub fn sync_server_config(&self, key_bindings: &KeyBindings) -> Result<(), ServiceError> {
        match self.multiplexer.server_config(key_bindings)? {
            Some(running) if !running.is_current() => {
                Ok(self.multiplexer.apply_config(key_bindings)?)
            }
            _ => Ok(()),
        }
    }

    pub fn reload_server_config(&self, key_bindings: &KeyBindings) -> Result<(), ServiceError> {
        match self.multiplexer.server_config(key_bindings)? {
            Some(_) => Ok(self.multiplexer.apply_config(key_bindings)?),
            None => Ok(()),
        }
    }

    pub fn delete_impact(
        &self,
        reconciliation: &Reconciliation,
        name: &str,
    ) -> Result<DeleteImpact, ServiceError> {
        let row = self.deletable_target(reconciliation, name)?;
        self.refuse_launch_inside(row.session())?;
        self.deletion.impact(&row)
    }

    pub fn delete(
        &self,
        reconciliation: &Reconciliation,
        name: &str,
        confirmation: &DeleteConfirmation,
    ) -> Result<DeleteOutcome, ServiceError> {
        let row = self.deletable_target(reconciliation, name)?;
        if !confirmation.forgets() {
            self.refuse_launch_inside(row.session())?;
        }
        let remaining = reconciliation.entries_without(name);
        self.deletion.delete(&row, &remaining, confirmation)
    }

    pub fn restart(
        &self,
        reconciliation: &Reconciliation,
        name: &str,
        config: &Config,
        config_path: &Path,
    ) -> Result<StartOutcome, ServiceError> {
        let row = self.quiet_target(reconciliation, name)?;
        self.sync_server_config(config.key_bindings())?;
        self.restart
            .restart(&row, reconciliation, config, config_path)
    }

    pub fn stop(&self, reconciliation: &Reconciliation, name: &str) -> Result<(), ServiceError> {
        let row = self.managed_target(reconciliation, name)?;
        self.refuse_own_session(&row, "stop")?;
        self.stopping.stop(&row)
    }

    pub fn merge_upstream(
        &self,
        reconciliation: &Reconciliation,
        name: &str,
        config: &Config,
    ) -> Result<MergeReport, ServiceError> {
        let row = self.managed_target(reconciliation, name)?;
        self.merging.merge(&row, reconciliation, config)
    }

    pub fn rename(
        &self,
        reconciliation: &Reconciliation,
        request: &RenameRequest,
        config: &Config,
    ) -> Result<Session, ServiceError> {
        let row = self.quiet_target(reconciliation, request.old_name())?;
        self.renaming.rename(&row, request, reconciliation, config)
    }

    fn deletable_target<'r>(
        &self,
        reconciliation: &'r Reconciliation,
        name: &str,
    ) -> Result<ManagedRow<'r>, ServiceError> {
        let row = self.managed_target(reconciliation, name)?;
        self.refuse_own_session(&row, "delete")?;
        Ok(row)
    }

    fn refuse_own_session(&self, row: &ManagedRow, action: &str) -> Result<(), ServiceError> {
        let session = row.session();
        if self.multiplexer.caller_session()?.as_deref() != Some(session.tmux_name()) {
            return Ok(());
        }
        Err(ServiceError::Conflict(format!(
            "session {} runs this Ringleader, {action} it from another terminal",
            session.name()
        )))
    }

    fn refuse_launch_inside(&self, session: &Session) -> Result<(), ServiceError> {
        let containment = WorktreeContainment::new(self.host.as_ref());
        let launched_inside = containment.holds(session.worktree_path(), &self.launch_directory);
        if session.runs_in_main_checkout() || !launched_inside {
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
        let socket = self.multiplexer.socket();
        match BusyWorktree::new(self.host.as_ref(), &socket).finding(session) {
            Some(finding) => Err(ServiceError::Conflict(finding)),
            None => Ok(()),
        }
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
