use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::creation_plan::CreateRequest;
use super::delete_impact::{DeleteConfirmation, DeleteImpact};
use super::delete_outcome::DeleteOutcome;
use super::diagnostic_paths::DiagnosticPaths;
use super::doctor::Doctor;
use super::doctor_report::DoctorReport;
use super::pending::Pending;
use super::ports::{ConfigError, ConfigSource, RegistryError, SessionRegistry};
use super::pull_request_catalog::PullRequestCatalog;
use super::reconciliation::Reconciliation;
use super::rename_request::RenameRequest;
use super::repo_catalog::{RepoCatalog, RepoDiscovery};
use super::repo_overrides::{AddRepoRequest, RepoAddition};
use super::service_context::{LoadedConfig, ServiceContext};
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use super::session_access::{SessionAccess, ShellOutcome};
use super::session_lifecycle::SessionLifecycle;
use super::session_merge::MergeReport;
use super::session_monitor::SessionMonitor;
use crate::domain::{
    Config, DiffStat, KeyBindings, PullRequest, Repo, Session, SessionObservation, SessionSummary,
};

pub struct SessionService {
    config: LoadedConfig,
    entries: Vec<Session>,
    registry: Arc<dyn SessionRegistry>,
    config_source: Arc<dyn ConfigSource>,
    monitor: SessionMonitor,
    lifecycle: SessionLifecycle,
    access: SessionAccess,
    catalog: RepoCatalog,
    pull_request_catalog: PullRequestCatalog,
    doctor: Doctor,
    paths: DiagnosticPaths,
}

impl SessionService {
    pub fn new(ports: ServicePorts, context: ServiceContext) -> Self {
        Self {
            monitor: SessionMonitor::new(&ports),
            lifecycle: SessionLifecycle::new(&ports, context.launch_directory()),
            access: SessionAccess::new(&ports, context.shell()),
            catalog: RepoCatalog::new(&ports, &context),
            pull_request_catalog: PullRequestCatalog::new(ports.repos().pull_requests()),
            doctor: Doctor::new(&ports, context.diagnostics().clone()),
            paths: context.diagnostics().clone(),
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

    pub fn discover_repos(&self) -> Pending<RepoDiscovery> {
        self.catalog.discover(self.config())
    }

    pub fn find_repos(&self) -> Pending<RepoDiscovery> {
        self.catalog.find_repos(self.config())
    }

    pub fn default_repo(&self) -> Pending<Option<Repo>> {
        self.catalog.default_repo(self.config(), &self.entries)
    }

    pub fn repo_at(&self, input: &str) -> Pending<Repo> {
        self.catalog.repo_at(input, self.config())
    }

    pub fn home_directory(&self) -> Option<&Path> {
        self.catalog.home()
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

    pub fn paths(&self) -> &DiagnosticPaths {
        &self.paths
    }

    pub fn restore_server_socket(&self) -> Result<Option<String>, ServiceError> {
        if self.read_only_reason().is_some() {
            return Ok(None);
        }
        self.lifecycle.restore_server_socket()
    }

    pub fn reapply_server_config(&self) -> Result<(), ServiceError> {
        if self.read_only_reason().is_some() {
            return Ok(());
        }
        self.lifecycle
            .sync_server_config(self.config().key_bindings())
    }

    pub fn shown_key_bindings(&self) -> KeyBindings {
        let configured = self.config().key_bindings().clone();
        if self.read_only_reason().is_none() {
            return configured;
        }
        self.monitor.server_key_bindings().unwrap_or(configured)
    }

    pub fn refuse_own_pane(&self) -> Result<(), ServiceError> {
        self.access.refuse_own_pane()
    }

    pub fn reload(&mut self) -> Result<(), ServiceError> {
        let config = self.config_source.load()?;
        let entries = self.registry.load()?;
        self.config = LoadedConfig::valid(config);
        self.entries = entries;
        Ok(())
    }

    pub fn observe(&mut self, persist: bool) -> Result<Vec<SessionSummary>, ServiceError> {
        let observations = self.monitor.observe_all()?;
        self.observe_with(observations, persist)
    }

    pub fn start_observing(&self) -> Pending<Vec<SessionObservation>> {
        self.monitor.start_observing()
    }

    pub fn observe_with(
        &mut self,
        observations: Vec<SessionObservation>,
        persist: bool,
    ) -> Result<Vec<SessionSummary>, ServiceError> {
        let reconciliation = self.reconcile_with(observations);
        if persist && reconciliation.changed() {
            self.registry.save(reconciliation.entries())?;
        }
        Ok(reconciliation.into_summaries())
    }

    pub fn diff_stats(&self, names: &[String]) -> Vec<(String, Result<DiffStat, ServiceError>)> {
        names
            .iter()
            .filter_map(|name| self.entry(name))
            .map(|session| {
                let diff = self.monitor.diff_stat(session, self.config());
                (session.name().to_string(), diff)
            })
            .collect()
    }

    pub fn start_diff(&self, name: &str) -> Option<Pending<DiffStat>> {
        let session = self.entry(name)?;
        Some(self.monitor.start_diff(session, self.config()))
    }

    pub fn start_preview(&self, tmux_name: &str) -> Pending<String> {
        self.monitor.start_preview(tmux_name)
    }

    pub fn find_pull_requests(
        &self,
        names: &[String],
    ) -> Pending<HashMap<String, Option<PullRequest>>> {
        let sessions: Vec<&Session> = names.iter().filter_map(|name| self.entry(name)).collect();
        self.pull_request_catalog.find(&sessions)
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

    pub fn rename_session(&mut self, old: &str, new: &str) -> Result<Session, ServiceError> {
        let reconciliation = self.reconcile()?;
        let request = RenameRequest::new(old, new);
        let renamed = self
            .lifecycle
            .rename(&reconciliation, &request, self.config())?;
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

    pub fn merge_upstream(&mut self, name: &str) -> Result<MergeReport, ServiceError> {
        let reconciliation = self.reconcile()?;
        self.lifecycle
            .merge_upstream(&reconciliation, name, self.config())
    }

    pub fn attach(&mut self, name: &str) -> Result<(), ServiceError> {
        let reconciliation = self.reconcile()?;
        self.access.attach(reconciliation.row(name)?)
    }

    pub fn open_shell(&mut self, name: &str) -> Result<ShellOutcome, ServiceError> {
        let reconciliation = self.reconcile()?;
        self.access.open_shell(reconciliation.row(name)?)
    }

    pub fn poll_startup_input(&self) {
        self.lifecycle.poll_startup_input();
    }

    pub fn startup_wait_note(&self) -> Option<&'static str> {
        self.lifecycle.startup_wait_note()
    }

    pub fn wait_startup_input(&self, stop: impl Fn() -> bool) {
        self.lifecycle.wait_startup_input(stop);
    }

    fn reconcile(&mut self) -> Result<Reconciliation, ServiceError> {
        let observations = self.monitor.observe_all()?;
        Ok(self.reconcile_with(observations))
    }

    fn reconcile_with(&mut self, observations: Vec<SessionObservation>) -> Reconciliation {
        let reconciliation =
            self.monitor
                .reconcile(&self.entries, self.config.config(), observations);
        self.entries = reconciliation.entries().to_vec();
        reconciliation
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
    use crate::test_support::{FakeMultiplexer, FakeSessionRegistry, ServiceFixture};

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
    fn a_restored_server_socket_gives_a_note_once() {
        let fixture =
            ServiceFixture::new().with_multiplexer(FakeMultiplexer::new().with_lost_socket());
        let service = fixture.service();

        let first = service.restore_server_socket().unwrap();
        let second = service.restore_server_socket().unwrap();

        assert!(first.unwrap().contains("recreated the socket"));
        assert_eq!(second, None);
    }

    #[test]
    fn a_reload_reads_the_config_again() {
        let fixture = ServiceFixture::new();
        let mut service = fixture.service();
        let replaced = ServiceFixture::config().with_agents(Vec::new());
        fixture.config_source().replace(replaced.clone());

        service.reload().unwrap();

        assert_eq!(service.config(), &replaced);
    }

    #[test]
    fn a_reload_of_a_valid_config_ends_read_only_mode() {
        let fixture = ServiceFixture::new().with_broken_config(ConfigError::Parse {
            path: PathBuf::from("/config/ringleader/config.toml"),
            detail: "expected `=`".to_string(),
        });
        let mut service = fixture.service();

        service.reload().unwrap();

        assert!(service.read_only_reason().is_none());
    }

    #[test]
    fn a_failed_registry_load_during_a_reload_keeps_the_previous_config() {
        let fixture = ServiceFixture::new();
        let mut service = fixture.service();
        fixture
            .config_source()
            .replace(ServiceFixture::config().with_agents(Vec::new()));
        fixture.registry().break_loads(RegistryError::Corrupt {
            path: PathBuf::from("/state/ringleader/sessions.json"),
            detail: "expected value".to_string(),
        });

        assert!(service.reload().is_err());
        assert_eq!(service.config(), &ServiceFixture::config());
    }

    #[test]
    fn a_caller_in_a_pane_of_the_own_server_is_refused_as_a_conflict() {
        let fixture =
            ServiceFixture::new().with_multiplexer(FakeMultiplexer::new().called_from_own_pane());

        let error = fixture.service().refuse_own_pane().unwrap_err();

        assert!(matches!(error, ServiceError::Conflict(_)));
    }

    #[test]
    fn a_drifted_server_config_is_reapplied_with_the_configured_keys() {
        let fixture = ServiceFixture::new()
            .with_config(ServiceFixture::config_with_alt_n())
            .with_multiplexer(FakeMultiplexer::new().with_drifted_config("C-q"));

        fixture.service().reapply_server_config().unwrap();

        assert!(
            fixture
                .multiplexer()
                .calls()
                .contains(&"apply_config C-q M-n M-k M-Up".to_string())
        );
    }

    #[test]
    fn a_matching_server_config_sends_no_change() {
        let fixture = ServiceFixture::new();

        fixture.service().reapply_server_config().unwrap();

        assert!(
            fixture
                .multiplexer()
                .calls()
                .iter()
                .all(|call| !call.starts_with("apply_config"))
        );
    }

    #[test]
    fn a_config_that_failed_to_load_skips_the_reapply() {
        let fixture = ServiceFixture::new()
            .with_broken_config(ConfigError::Parse {
                path: PathBuf::from("/config/ringleader/config.toml"),
                detail: "expected `=`".to_string(),
            })
            .with_multiplexer(FakeMultiplexer::new().with_drifted_config("M-x"));

        fixture.service().reapply_server_config().unwrap();

        assert!(fixture.multiplexer().calls().is_empty());
    }

    #[test]
    fn a_read_only_service_shows_the_key_the_running_server_reports() {
        let fixture = ServiceFixture::new()
            .with_broken_config(ConfigError::Parse {
                path: PathBuf::from("/config/ringleader/config.toml"),
                detail: "expected `=`".to_string(),
            })
            .with_multiplexer(FakeMultiplexer::new().with_drifted_config("M-x"));

        let shown = fixture.service().shown_key_bindings();
        assert_eq!(shown.return_key().to_string(), "M-x");
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

        assert!(matches!(error, ServiceError::Failed(_)));
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
