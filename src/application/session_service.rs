use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::creation_plan::CreateRequest;
use super::delete_impact::{DeleteConfirmation, DeleteImpact};
use super::delete_outcome::DeleteOutcome;
use super::diagnostic_paths::DiagnosticPaths;
use super::doctor::Doctor;
use super::doctor_report::DoctorReport;
use super::merge_report::MergeReport;
use super::pending::Pending;
use super::ports::{ConfigError, ConfigSource, RegistryError, SessionRegistry};
use super::pull_request_catalog::PullRequestCatalog;
use super::reconciliation::Reconciliation;
use super::rename_request::RenameRequest;
use super::repo_catalog::{RepoCatalog, RepoDiscovery};
use super::repo_overrides::{AddRepoRequest, RepoAddition};
use super::restart_report::RestartReport;
use super::service_context::{LoadedConfig, ServiceContext};
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use super::session_access::{SessionAccess, ShellOutcome};
use super::session_lifecycle::SessionLifecycle;
use super::session_monitor::SessionMonitor;
use super::start_outcome::StartOutcome;
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
        self.lifecycle.restore_server_socket()
    }

    pub fn reapply_server_config(&self) -> Result<(), ServiceError> {
        if self.read_only_reason().is_some() {
            return Ok(());
        }
        self.lifecycle
            .sync_server_config(self.config().key_bindings())
    }

    pub fn reload_server_config(&self) -> Result<(), ServiceError> {
        if self.read_only_reason().is_some() {
            return Ok(());
        }
        self.lifecycle
            .reload_server_config(self.config().key_bindings())
    }

    pub fn shown_key_bindings(&self) -> KeyBindings {
        self.monitor
            .server_key_bindings()
            .unwrap_or_else(|| self.config().key_bindings().clone())
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
                let diff = self
                    .monitor
                    .diff_stat(session, &self.entries, self.config());
                (session.name().to_string(), diff)
            })
            .collect()
    }

    pub fn start_diff(&self, name: &str) -> Option<Pending<DiffStat>> {
        let session = self.entry(name)?;
        Some(
            self.monitor
                .start_diff(session, &self.entries, self.config()),
        )
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

    pub fn create_session(
        &mut self,
        request: &CreateRequest,
    ) -> Result<StartOutcome, ServiceError> {
        let reconciliation = self.reconcile()?;
        let config_path = self.config_path();
        let outcome =
            self.lifecycle
                .create(request, &reconciliation, self.config(), &config_path)?;
        self.entries.push(outcome.session().clone());
        Ok(outcome)
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

    pub fn restart_session(&mut self, name: &str) -> Result<StartOutcome, ServiceError> {
        let recovery = self.lifecycle.restore_server_socket()?;
        let reconciliation = self.reconcile()?;
        let config_path = self.config_path();
        let outcome = self
            .lifecycle
            .restart(&reconciliation, name, self.config(), &config_path)?;
        self.entries = reconciliation.entries_with(name, outcome.session().clone());
        Ok(outcome.with_recovery(recovery))
    }

    pub fn restart_all(&mut self, stop: impl Fn() -> bool) -> Result<RestartReport, ServiceError> {
        let names = self.reconcile()?.restartable_names();
        let restarts = names
            .into_iter()
            .take_while(|_| !stop())
            .map(|name| {
                let restart = self.restart_session(&name);
                (name, restart)
            })
            .collect();
        Ok(RestartReport::new(restarts))
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

    pub fn open_terminal(&self, name: &str) -> Result<(), ServiceError> {
        let session = self
            .entry(name)
            .ok_or_else(|| ServiceError::NotFound(format!("no managed session named {name}")))?;
        self.access.open_terminal(session)
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
    fn a_read_only_service_still_recreates_a_lost_socket() {
        let fixture = ServiceFixture::new()
            .with_broken_config(ConfigError::Parse {
                path: PathBuf::from("/config/ringleader/config.toml"),
                detail: "expected `=`".to_string(),
            })
            .with_multiplexer(FakeMultiplexer::new().with_lost_socket());

        let note = fixture.service().restore_server_socket().unwrap();

        assert!(note.unwrap().contains("recreated the socket"));
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
                .contains(&"apply_config C-q M-n M-k M-Up M-l".to_string())
        );
    }

    #[test]
    fn a_reload_resources_a_server_config_whose_hash_matches() {
        let fixture = ServiceFixture::new();

        fixture.service().reload_server_config().unwrap();

        assert!(
            fixture
                .multiplexer()
                .calls()
                .contains(&"apply_config C-q M-j M-Down M-k M-Up M-l".to_string())
        );
    }

    #[test]
    fn a_reload_without_a_running_server_sends_no_change() {
        let fixture = ServiceFixture::new()
            .with_multiplexer(FakeMultiplexer::new().with_versions("3.6a", None));

        fixture.service().reload_server_config().unwrap();

        assert!(
            fixture
                .multiplexer()
                .calls()
                .iter()
                .all(|call| !call.starts_with("apply_config"))
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
    fn a_server_still_binding_another_key_shows_the_key_it_binds() {
        let fixture = ServiceFixture::new()
            .with_multiplexer(FakeMultiplexer::new().with_drifted_config("M-x"));

        assert_eq!(
            fixture
                .service()
                .shown_key_bindings()
                .return_key()
                .to_string(),
            "M-x"
        );
    }

    #[test]
    fn without_a_running_server_the_configured_key_is_shown() {
        let fixture = ServiceFixture::new()
            .with_multiplexer(FakeMultiplexer::new().with_versions("3.6a", None));

        assert_eq!(
            fixture.service().shown_key_bindings(),
            *ServiceFixture::config().key_bindings()
        );
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

    struct RestartAllFixture;

    impl RestartAllFixture {
        const STOPPED: [&'static str; 2] = ["alpha", "echo"];
        const EXITED: &'static str = "bravo";
        const WAITING: &'static str = "charlie";
        const WORKING: &'static str = "delta";

        fn sessions() -> Vec<Session> {
            [
                Self::STOPPED[0],
                Self::EXITED,
                Self::WAITING,
                Self::WORKING,
                Self::STOPPED[1],
            ]
            .map(ServiceFixture::session)
            .to_vec()
        }

        fn mixed() -> ServiceFixture {
            let sessions = Self::sessions();
            let all: Vec<&Session> = sessions.iter().collect();
            Self::with_worktrees_of(&sessions, &all)
        }

        fn with_worktrees_of(sessions: &[Session], present: &[&Session]) -> ServiceFixture {
            ServiceFixture::new()
                .with_sessions(sessions)
                .with_workspace(ServiceFixture::workspace_for(present))
                .with_host(ServiceFixture::host_for(present))
                .with_multiplexer(Self::live_sessions())
        }

        fn live_sessions() -> FakeMultiplexer {
            let stranger = ServiceFixture::session("mystery");
            FakeMultiplexer::new()
                .with_live(ServiceFixture::exited(&ServiceFixture::session(
                    Self::EXITED,
                )))
                .with_live(ServiceFixture::waiting(&ServiceFixture::session(
                    Self::WAITING,
                )))
                .with_live(ServiceFixture::working(&ServiceFixture::session(
                    Self::WORKING,
                )))
                .with_live(ServiceFixture::waiting(&stranger).without_metadata())
        }

        fn restarted_names(report: &RestartReport) -> Vec<String> {
            report
                .restarted()
                .map(|outcome| outcome.session().name().as_str().to_string())
                .collect()
        }

        fn recreated_tmux_names(fixture: &ServiceFixture) -> Vec<String> {
            fixture
                .multiplexer()
                .specs()
                .iter()
                .map(|spec| spec.tmux_name().to_string())
                .collect()
        }
    }

    #[test]
    fn restart_all_restarts_every_exited_and_stopped_session_in_name_order() {
        let fixture = RestartAllFixture::mixed();

        let report = fixture.service().restart_all(|| false).unwrap();

        assert_eq!(
            RestartAllFixture::restarted_names(&report),
            ["alpha", "bravo", "echo"]
        );
        assert_eq!(report.failures().count(), 0);
    }

    #[test]
    fn restart_all_recreates_only_the_stopped_sessions() {
        let fixture = RestartAllFixture::mixed();

        fixture.service().restart_all(|| false).unwrap();

        assert_eq!(
            RestartAllFixture::recreated_tmux_names(&fixture),
            ["ringleader-alpha", "ringleader-echo"]
        );
    }

    #[test]
    fn restart_all_records_every_new_socket_because_a_shared_reconciliation_drops_earlier_ones() {
        let recorded = Path::new(ServiceFixture::OTHER_RUNTIME_SOCKET);
        let sessions = RestartAllFixture::STOPPED
            .map(|name| ServiceFixture::session(name).on_socket(recorded))
            .to_vec();
        let present: Vec<&Session> = sessions.iter().collect();
        let fixture = RestartAllFixture::with_worktrees_of(&sessions, &present)
            .with_multiplexer(FakeMultiplexer::new());

        fixture.service().restart_all(|| false).unwrap();

        let entries = fixture.registry().sessions();
        let sockets: Vec<&Path> = entries.iter().map(Session::socket_path).collect();
        let private_socket = Path::new(FakeMultiplexer::SOCKET);
        assert_eq!(sockets, [private_socket, private_socket]);
    }

    #[test]
    fn restart_all_keeps_going_past_a_failed_restart_and_reports_it() {
        let sessions = RestartAllFixture::sessions();
        let without_alpha: Vec<&Session> = sessions.iter().skip(1).collect();
        let fixture = RestartAllFixture::with_worktrees_of(&sessions, &without_alpha);

        let report = fixture.service().restart_all(|| false).unwrap();

        let failures: Vec<(&str, &ServiceError)> = report.failures().collect();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].0, "alpha");
        assert!(failures[0].1.to_string().contains("is missing"));
        assert_eq!(
            RestartAllFixture::restarted_names(&report),
            ["bravo", "echo"]
        );
    }

    #[test]
    fn restart_all_skips_the_rest_once_stop_fires() {
        let fixture = RestartAllFixture::mixed();
        let checks = std::cell::Cell::new(0);

        let report = fixture
            .service()
            .restart_all(|| {
                checks.set(checks.get() + 1);
                checks.get() > 1
            })
            .unwrap();

        assert_eq!(RestartAllFixture::restarted_names(&report), ["alpha"]);
    }

    #[test]
    fn restart_all_without_an_exited_or_stopped_session_reports_nothing() {
        let fixture = ServiceFixture::new();

        let report = fixture.service().restart_all(|| false).unwrap();

        assert!(report.is_empty());
    }

    #[test]
    fn restart_all_types_the_startup_input_of_every_restarted_agent() {
        let fixture = RestartAllFixture::mixed().with_config(ServiceFixture::config_with_input());

        fixture.service().restart_all(|| false).unwrap();

        for tmux_name in ["ringleader-alpha", "ringleader-bravo", "ringleader-echo"] {
            assert!(
                fixture
                    .multiplexer()
                    .wait_for_typing(tmux_name, std::time::Duration::from_secs(2)),
                "{tmux_name}"
            );
        }
    }

    #[test]
    fn restart_all_notes_a_recreated_socket_on_the_first_restart_only() {
        let fixture = RestartAllFixture::mixed()
            .with_multiplexer(RestartAllFixture::live_sessions().with_lost_socket());

        let report = fixture.service().restart_all(|| false).unwrap();

        let recoveries: Vec<bool> = report
            .restarted()
            .map(|outcome| outcome.recovery().is_some())
            .collect();
        assert_eq!(recoveries, [true, false, false]);
    }
}
