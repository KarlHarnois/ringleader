use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{
    FakeClock, FakeConfigSource, FakeHostProbe, FakeMultiplexer, FakeRepoScanner,
    FakeReturnBinding, FakeSessionRegistry, FakeWorkspace,
};
use crate::application::ports::{ConfigError, WorktreeEntry};
use crate::application::{
    CorruptRegistry, LoadedConfig, RepoPorts, ServiceContext, ServicePorts, SessionService,
    StatePorts, TmuxPorts,
};
use crate::domain::{
    Agent, AgentPane, Config, NamingScheme, Registration, Repo, RepoName, Session, SessionMetadata,
    SessionName, SessionObservation, SessionRepo, SessionWorktree, TmuxAddress,
};

pub struct ServiceFixture {
    registry: Arc<FakeSessionRegistry>,
    workspace: Arc<FakeWorkspace>,
    multiplexer: Arc<FakeMultiplexer>,
    binding: Arc<FakeReturnBinding>,
    config_source: Arc<FakeConfigSource>,
    scanner: Arc<FakeRepoScanner>,
    host: Arc<FakeHostProbe>,
    clock: Arc<FakeClock>,
    config_error: Option<ConfigError>,
    inside_tmux: bool,
    shell: String,
    home: Option<PathBuf>,
}

impl ServiceFixture {
    pub const NOW: u64 = 1_700_000_100;
    pub const CREATED_AT: u64 = 1_700_000_000;
    pub const REPO: &'static str = "/repos/example-project";
    pub const DATA_DIR: &'static str = "/data/ringleader";
    pub const HOME: &'static str = "/home/example";
    pub const WORKTREE_ROOT: &'static str = "/data/ringleader/worktrees";
    pub const TIP: &'static str = "feedfacefeedfacefeedfacefeedfacefeedface";
    pub const IDLE_SECONDS: u64 = 3;
    pub const SHELL: &'static str = "/bin/example-shell";

    pub fn new() -> Self {
        Self {
            registry: Arc::new(FakeSessionRegistry::empty()),
            workspace: Arc::new(FakeWorkspace::default()),
            multiplexer: Arc::new(FakeMultiplexer::new()),
            binding: Arc::new(FakeReturnBinding::default()),
            config_source: Arc::new(FakeConfigSource::with_config(Self::config())),
            scanner: Arc::new(FakeRepoScanner::finding(vec![Self::repo()])),
            host: Arc::new(Self::host_for(&[])),
            clock: Arc::new(FakeClock::at(Self::NOW)),
            config_error: None,
            inside_tmux: false,
            shell: Self::SHELL.to_string(),
            home: Some(PathBuf::from(Self::HOME)),
        }
    }

    pub fn with_registry(self, registry: FakeSessionRegistry) -> Self {
        Self {
            registry: Arc::new(registry),
            ..self
        }
    }

    pub fn with_sessions(self, sessions: &[Session]) -> Self {
        self.with_registry(FakeSessionRegistry::with_sessions(sessions.to_vec()))
    }

    pub fn with_workspace(self, workspace: FakeWorkspace) -> Self {
        Self {
            workspace: Arc::new(workspace),
            ..self
        }
    }

    pub fn with_multiplexer(self, multiplexer: FakeMultiplexer) -> Self {
        Self {
            multiplexer: Arc::new(multiplexer),
            ..self
        }
    }

    pub fn with_binding(self, binding: FakeReturnBinding) -> Self {
        Self {
            binding: Arc::new(binding),
            ..self
        }
    }

    pub fn with_config_source(self, config_source: FakeConfigSource) -> Self {
        Self {
            config_source: Arc::new(config_source),
            ..self
        }
    }

    pub fn with_config(self, config: Config) -> Self {
        self.with_config_source(FakeConfigSource::with_config(config))
    }

    pub fn with_broken_config(self, error: ConfigError) -> Self {
        Self {
            config_error: Some(error),
            ..self
        }
    }

    pub fn with_scanner(self, scanner: FakeRepoScanner) -> Self {
        Self {
            scanner: Arc::new(scanner),
            ..self
        }
    }

    pub fn with_host(self, host: FakeHostProbe) -> Self {
        Self {
            host: Arc::new(host),
            ..self
        }
    }

    pub fn without_home(self) -> Self {
        Self { home: None, ..self }
    }

    pub fn inside_tmux(self) -> Self {
        Self {
            inside_tmux: true,
            ..self
        }
    }

    pub fn service(&self) -> SessionService {
        let mut service = self.unloaded_service();
        service
            .load_registry(CorruptRegistry::Report)
            .expect("the fixture registry loads");
        service
    }

    pub fn unloaded_service(&self) -> SessionService {
        SessionService::new(self.ports(), self.context())
    }

    pub fn registry(&self) -> &FakeSessionRegistry {
        &self.registry
    }

    pub fn workspace(&self) -> &FakeWorkspace {
        &self.workspace
    }

    pub fn multiplexer(&self) -> &FakeMultiplexer {
        &self.multiplexer
    }

    pub fn binding(&self) -> &FakeReturnBinding {
        &self.binding
    }

    pub fn shared_binding(&self) -> Arc<FakeReturnBinding> {
        Arc::clone(&self.binding)
    }

    pub fn config_source(&self) -> &FakeConfigSource {
        &self.config_source
    }

    pub fn scanner(&self) -> &FakeRepoScanner {
        &self.scanner
    }

    pub fn host(&self) -> &FakeHostProbe {
        &self.host
    }

    pub fn repo() -> Repo {
        Repo::discovered(
            RepoName::parse("example-project").unwrap(),
            Path::new(Self::REPO),
        )
    }

    pub fn agent() -> Agent {
        Agent::new("claude", "claude")
            .with_env_names(&["ANTHROPIC_API_KEY".to_string()])
            .with_prompt_args(&["{prompt}".to_string()])
    }

    pub fn config() -> Config {
        Config::defaults(Path::new(Self::WORKTREE_ROOT)).with_agents(vec![Self::agent()])
    }

    pub fn naming() -> NamingScheme {
        Self::config().naming().clone()
    }

    pub fn session(name: &str) -> Session {
        Self::session_at(name, Self::REPO, Self::CREATED_AT)
    }

    pub fn session_at(name: &str, repo_path: &str, created_at: u64) -> Session {
        let name = SessionName::new(name).unwrap();
        let naming = Self::naming();
        let repo_path = Path::new(repo_path);
        let repo_name = RepoName::of_directory(repo_path).unwrap();
        let metadata = SessionMetadata::new(
            SessionRepo::new(repo_name.clone(), repo_path),
            SessionWorktree::new(
                &naming.worktree_path(&repo_name, &name),
                &naming.branch_name(&name),
                FakeWorkspace::DEFAULT_START,
            ),
            "claude",
        );
        let tmux = TmuxAddress::new(
            &naming.tmux_session_name(&name),
            Path::new(FakeMultiplexer::SOCKET),
        );
        Session::new(name, metadata, Registration::new(tmux, naming, created_at))
    }

    pub fn waiting(session: &Session) -> SessionObservation {
        Self::observed(session, AgentPane::live(Self::NOW - 600))
    }

    pub fn working(session: &Session) -> SessionObservation {
        Self::observed(session, AgentPane::live(Self::NOW))
    }

    pub fn exited(session: &Session) -> SessionObservation {
        Self::observed(session, AgentPane::dead(Self::NOW - 600))
    }

    pub fn observed(session: &Session, pane: AgentPane) -> SessionObservation {
        SessionObservation::new(
            TmuxAddress::new(session.tmux_name(), session.socket_path()),
            Self::CREATED_AT,
        )
        .with_agent_pane(pane)
        .with_metadata(session.metadata().clone())
    }

    pub fn host_for(sessions: &[&Session]) -> FakeHostProbe {
        let host = FakeHostProbe::default()
            .with_command("claude")
            .with_directory(Path::new(Self::REPO))
            .with_directory(Path::new(Self::DATA_DIR));
        for session in sessions {
            host.add_directory(session.worktree_path());
        }
        sessions.iter().fold(host, |host, session| {
            host.with_file(&session.worktree_path().join(".git"))
        })
    }

    pub fn workspace_for(sessions: &[&Session]) -> FakeWorkspace {
        let repo = Path::new(Self::REPO);
        let workspace = FakeWorkspace::default()
            .with_worktree(repo, WorktreeEntry::new(repo).on_branch("main").main());
        sessions.iter().fold(workspace, |workspace, session| {
            workspace
                .with_worktree(
                    repo,
                    WorktreeEntry::new(session.worktree_path())
                        .on_branch(session.branch())
                        .at_head(Self::TIP),
                )
                .with_branch(repo, session.branch(), Self::TIP)
                .with_marker(repo, session.branch())
        })
    }

    pub fn worktree_path(name: &str) -> PathBuf {
        Self::session(name).worktree_path().to_path_buf()
    }

    fn ports(&self) -> ServicePorts {
        ServicePorts::new(
            StatePorts::new(
                self.registry.clone(),
                self.config_source.clone(),
                self.clock.clone(),
            ),
            RepoPorts::new(
                self.workspace.clone(),
                self.scanner.clone(),
                self.host.clone(),
            ),
            TmuxPorts::new(self.multiplexer.clone(), self.binding.clone()),
        )
    }

    fn context(&self) -> ServiceContext {
        let config = self.config_source.config();
        let loaded = match &self.config_error {
            Some(error) => LoadedConfig::failed(config, error.clone()),
            None => LoadedConfig::valid(config),
        };
        ServiceContext::new(loaded, Path::new(Self::REPO))
            .with_inside_tmux(self.inside_tmux)
            .with_home(self.home.as_deref())
            .with_shell(&self.shell)
    }
}
