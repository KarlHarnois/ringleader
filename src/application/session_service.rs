use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::creation_plan::CreateRequest;
use super::delete_impact::{DeleteConfirmation, DeleteImpact};
use super::delete_outcome::DeleteOutcome;
use super::doctor::Doctor;
use super::doctor_report::DoctorReport;
use super::pending::Pending;
use super::ports::{ConfigError, ConfigSource, RegistryError, SessionRegistry};
use super::reconciliation::Reconciliation;
use super::repo_catalog::{RepoCatalog, RepoDiscovery};
use super::repo_overrides::{AddRepoRequest, RepoAddition};
use super::service_context::{LoadedConfig, ServiceContext};
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use super::session_access::{AttachOutcome, ReturnMode, SessionAccess};
use super::session_lifecycle::SessionLifecycle;
use super::session_monitor::SessionMonitor;
use crate::domain::{Config, DiffStat, Session, SessionSummary};

pub struct SessionService {
    config: LoadedConfig,
    entries: Vec<Session>,
    registry: Arc<dyn SessionRegistry>,
    config_source: Arc<dyn ConfigSource>,
    monitor: SessionMonitor,
    lifecycle: SessionLifecycle,
    access: SessionAccess,
    catalog: RepoCatalog,
    doctor: Doctor,
    inside_tmux: bool,
}

impl SessionService {
    pub fn new(ports: ServicePorts, context: ServiceContext) -> Self {
        Self {
            monitor: SessionMonitor::new(&ports),
            lifecycle: SessionLifecycle::new(&ports),
            access: SessionAccess::new(ports.tmux(), context.shell()),
            catalog: RepoCatalog::new(&ports, &context),
            doctor: Doctor::new(&ports, context.diagnostics().clone()),
            inside_tmux: context.is_inside_tmux(),
            config: context.into_config(),
            entries: Vec::new(),
            registry: ports.state().registry(),
            config_source: ports.state().config_source(),
        }
    }

    pub fn load_registry(
        &mut self,
        recovery: CorruptRegistry,
    ) -> Result<Option<String>, ServiceError> {
        match (self.registry.load(), recovery) {
            (Ok(entries), _) => {
                self.entries = entries;
                Ok(None)
            }
            (Err(RegistryError::Corrupt { path, detail }), CorruptRegistry::MoveAside) => {
                let moved_to = self.registry.move_aside_corrupt()?;
                self.entries = Vec::new();
                Ok(Some(format!(
                    "{} was corrupt ({detail}), moved it to {}",
                    path.display(),
                    moved_to.display()
                )))
            }
            (Err(error), _) => Err(error.into()),
        }
    }

    pub fn config(&self) -> &Config {
        self.config.config()
    }

    pub fn config_path(&self) -> PathBuf {
        self.config_source.path()
    }

    pub fn read_only_reason(&self) -> Option<&ConfigError> {
        self.config.error()
    }

    pub fn launch_directory(&self) -> &Path {
        self.catalog.launch_directory()
    }

    pub fn discover_repos(&self, scope_directory: &Path) -> Pending<RepoDiscovery> {
        self.catalog.discover(scope_directory, self.config())
    }

    pub fn read_typed_path(&self, input: &str) -> Pending<PathBuf> {
        self.catalog.read_typed_path(input)
    }

    pub fn add_repo(&self, request: AddRepoRequest) -> Pending<RepoAddition> {
        self.catalog.add_repo(request, self.config())
    }

    pub fn remove_repo(&self, name: &str) -> Pending<String> {
        self.catalog.remove_repo(name, self.config(), &self.entries)
    }

    pub fn validated_top_level(&self, path: &Path) -> Pending<PathBuf> {
        self.catalog.validated_top_level(path)
    }

    pub fn is_inside_tmux(&self) -> bool {
        self.inside_tmux
    }

    pub fn reload(&mut self) -> Result<(), ServiceError> {
        let config = self.config_source.load()?;
        self.config = LoadedConfig::valid(config);
        self.entries = self.registry.load()?;
        Ok(())
    }

    pub fn observe(&mut self, persist: bool) -> Result<Vec<SessionSummary>, ServiceError> {
        let reconciliation = self.reconcile()?;
        if persist && reconciliation.changed() {
            self.registry.save(reconciliation.entries())?;
        }
        Ok(reconciliation.into_summaries())
    }

    pub fn diff_stats(&self, names: &[String]) -> Vec<(String, Result<DiffStat, ServiceError>)> {
        names
            .iter()
            .filter_map(|name| self.entry(name))
            .map(|session| (session.name().to_string(), self.monitor.diff_stat(session)))
            .collect()
    }

    pub fn preview(&self, tmux_name: &str) -> Result<String, ServiceError> {
        self.monitor.preview(tmux_name)
    }

    pub fn create_session(&mut self, request: &CreateRequest) -> Result<Session, ServiceError> {
        let reconciliation = self.reconcile()?;
        let config_path = self.config_path();
        let session =
            self.lifecycle
                .create(request, &reconciliation, self.config(), &config_path)?;
        self.entries.push(session.clone());
        Ok(session)
    }

    pub fn delete_impact(&mut self, name: &str) -> Result<DeleteImpact, ServiceError> {
        let reconciliation = self.reconcile()?;
        self.lifecycle.delete_impact(&reconciliation, name)
    }

    pub fn delete_session(
        &mut self,
        name: &str,
        confirmation: &DeleteConfirmation,
    ) -> Result<DeleteOutcome, ServiceError> {
        let reconciliation = self.reconcile()?;
        let outcome = self.lifecycle.delete(&reconciliation, name, confirmation)?;
        self.entries.retain(|entry| entry.name().as_str() != name);
        Ok(outcome)
    }

    pub fn rename_session(
        &mut self,
        old: &str,
        new: &str,
        keep_branch: bool,
    ) -> Result<Session, ServiceError> {
        let reconciliation = self.reconcile()?;
        let renamed = self
            .lifecycle
            .rename(&reconciliation, old, new, keep_branch)?;
        self.entries = reconciliation.entries_with(old, renamed.clone());
        Ok(renamed)
    }

    pub fn doctor(&self) -> DoctorReport {
        self.doctor
            .examine(self.config(), self.read_only_reason(), &self.config_path())
    }

    pub fn restart_session(&mut self, name: &str) -> Result<(), ServiceError> {
        let reconciliation = self.reconcile()?;
        let config_path = self.config_path();
        let restarted =
            self.lifecycle
                .restart(&reconciliation, name, self.config(), &config_path)?;
        self.entries = reconciliation.entries_with(name, restarted);
        Ok(())
    }

    pub fn attach(&mut self, name: &str, mode: ReturnMode) -> Result<AttachOutcome, ServiceError> {
        let reconciliation = self.reconcile()?;
        self.access.attach(reconciliation.row(name)?, mode)
    }

    pub fn ring_bell(&self) -> Result<(), ServiceError> {
        self.monitor.ring_bell()
    }

    fn reconcile(&mut self) -> Result<Reconciliation, ServiceError> {
        let reconciliation = self
            .monitor
            .reconcile(&self.entries, self.config.config())?;
        self.entries = reconciliation.entries().to_vec();
        Ok(reconciliation)
    }

    fn entry(&self, name: &str) -> Option<&Session> {
        self.entries
            .iter()
            .find(|entry| entry.name().as_str() == name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorruptRegistry {
    MoveAside,
    Report,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{FakeSessionRegistry, ServiceFixture};

    #[test]
    fn config_returns_the_loaded_config() {
        let fixture = ServiceFixture::new();

        assert_eq!(fixture.service().config(), &ServiceFixture::config());
    }

    #[test]
    fn a_broken_config_makes_the_service_read_only() {
        let fixture = ServiceFixture::new().with_broken_config(ConfigError::Parse {
            path: PathBuf::from("/config/ringleader/config.toml"),
            detail: "expected `=`".to_string(),
        });

        assert!(fixture.service().read_only_reason().is_some());
    }

    #[test]
    fn a_lock_holder_moves_a_corrupt_registry_aside() {
        let registry = FakeSessionRegistry::empty().failing_load(RegistryError::Corrupt {
            path: PathBuf::from("/state/ringleader/sessions.json"),
            detail: "expected value".to_string(),
        });
        let fixture = ServiceFixture::new().with_registry(registry);
        let mut service = fixture.unloaded_service();

        let warning = service.load_registry(CorruptRegistry::MoveAside).unwrap();

        assert!(warning.unwrap().contains("sessions.json.corrupt-1"));
        assert!(fixture.registry().was_moved_aside());
    }

    #[test]
    fn a_reader_reports_a_corrupt_registry_and_changes_nothing() {
        let registry = FakeSessionRegistry::empty().failing_load(RegistryError::Corrupt {
            path: PathBuf::from("/state/ringleader/sessions.json"),
            detail: "expected value".to_string(),
        });
        let fixture = ServiceFixture::new().with_registry(registry);
        let mut service = fixture.unloaded_service();

        let error = service.load_registry(CorruptRegistry::Report).unwrap_err();

        assert_eq!(error.exit_code(), 1);
        assert!(!fixture.registry().was_moved_aside());
    }

    #[test]
    fn a_newer_registry_is_refused_by_every_reader() {
        let registry = FakeSessionRegistry::empty().failing_load(RegistryError::NewerSchema {
            found: 9,
            supported: 1,
        });
        let fixture = ServiceFixture::new().with_registry(registry);
        let mut service = fixture.unloaded_service();

        let error = service
            .load_registry(CorruptRegistry::MoveAside)
            .unwrap_err();

        assert!(error.to_string().contains("Upgrade Ringleader"));
        assert!(!fixture.registry().was_moved_aside());
    }
}
