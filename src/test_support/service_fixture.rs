use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{
    FakeClock, FakeConfigSource, FakeDirectoryEnvironment, FakeHostProbe, FakeMultiplexer,
    FakePullRequests, FakeRepoScanner, FakeScratchSpace, FakeSessionRegistry, FakeWorkspace,
};
use crate::application::ports::{
    ConfigError, MergeOutcome, MultiplexerError, ScratchSpace, WorktreeEntry,
};
use crate::application::{
    CorruptRegistry, DiagnosticPaths, LoadedConfig, RepoPorts, ServiceContext, ServicePorts,
    SessionService, StatePorts, TmuxPorts,
};
use crate::domain::{
    Agent, AgentPane, BaseBranch, Config, HotKey, KeyBindings, NamingScheme, PullRequest,
    PullRequestState, Registration, Repo, RepoName, Session, SessionMetadata, SessionName,
    SessionObservation, SessionRepo, SessionState, SessionSummary, SessionWorktree, TmuxAddress,
};

pub struct ServiceFixture {
    registry: Arc<FakeSessionRegistry>,
    workspace: Arc<FakeWorkspace>,
    diff_workspace: Option<Arc<FakeWorkspace>>,
    multiplexer: Arc<FakeMultiplexer>,
    config_source: Arc<FakeConfigSource>,
    scanner: Arc<FakeRepoScanner>,
    host: Arc<FakeHostProbe>,
    pull_requests: Arc<FakePullRequests>,
    clock: Arc<FakeClock>,
    scratch_space: Arc<FakeScratchSpace>,
    directory_environment: Arc<FakeDirectoryEnvironment>,
    config_error: Option<ConfigError>,
    launch_directory: PathBuf,
    diagnostics: DiagnosticPaths,
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
    pub const WORKTREE_PROCESS: u32 = 4_000_042;
    pub const OTHER_RUNTIME_SOCKET: &'static str =
        "/home/example/.local/state/ringleader/tmux/tmux.sock";
    pub const STATE_DIRECTORY: &'static str = "/state/ringleader";
    pub const TMUX_CONFIG: &'static str = "/state/ringleader/tmux.conf";
    pub const LOG_FILE: &'static str = "/state/ringleader/app.log";
    pub const ENVRC: &'static str = "use flake\n";

    pub fn new() -> Self {
        Self {
            registry: Arc::new(FakeSessionRegistry::empty()),
            workspace: Arc::new(FakeWorkspace::default()),
            diff_workspace: None,
            multiplexer: Arc::new(FakeMultiplexer::new()),
            config_source: Arc::new(FakeConfigSource::with_config(Self::config())),
            scanner: Arc::new(FakeRepoScanner::finding(vec![Self::repo()])),
            host: Arc::new(Self::host_for(&[])),
            pull_requests: Arc::new(FakePullRequests::new()),
            clock: Arc::new(FakeClock::at(Self::NOW)),
            scratch_space: Arc::new(FakeScratchSpace::default()),
            directory_environment: Arc::new(FakeDirectoryEnvironment::default()),
            config_error: None,
            launch_directory: PathBuf::from(Self::REPO),
            diagnostics: Self::diagnostics(),
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

    pub fn with_diff_workspace(self, diff_workspace: FakeWorkspace) -> Self {
        Self {
            diff_workspace: Some(Arc::new(diff_workspace)),
            ..self
        }
    }

    pub fn with_multiplexer(self, multiplexer: FakeMultiplexer) -> Self {
        Self {
            multiplexer: Arc::new(multiplexer),
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

    pub fn with_pull_requests(self, pull_requests: FakePullRequests) -> Self {
        Self {
            pull_requests: Arc::new(pull_requests),
            ..self
        }
    }

    pub fn with_scratch_space(self, scratch_space: FakeScratchSpace) -> Self {
        Self {
            scratch_space: Arc::new(scratch_space),
            ..self
        }
    }

    pub fn with_directory_environment(self, environment: FakeDirectoryEnvironment) -> Self {
        Self {
            directory_environment: Arc::new(environment),
            ..self
        }
    }

    pub fn without_home(self) -> Self {
        Self { home: None, ..self }
    }

    pub fn with_diagnostics(self, diagnostics: DiagnosticPaths) -> Self {
        Self {
            diagnostics,
            ..self
        }
    }

    pub fn diagnostics() -> DiagnosticPaths {
        DiagnosticPaths::new("tmux", "git", Path::new(Self::STATE_DIRECTORY))
            .with_tmux_server(
                Path::new(FakeMultiplexer::SOCKET),
                Path::new(Self::TMUX_CONFIG),
            )
            .with_log_file(Path::new(Self::LOG_FILE))
    }

    pub fn launched_in(self, directory: &Path) -> Self {
        Self {
            launch_directory: directory.to_path_buf(),
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

    pub fn shared_multiplexer(&self) -> Arc<FakeMultiplexer> {
        Arc::clone(&self.multiplexer)
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

    pub fn pull_requests(&self) -> &FakePullRequests {
        &self.pull_requests
    }

    pub fn scratch_space(&self) -> &FakeScratchSpace {
        &self.scratch_space
    }

    pub fn directory_environment(&self) -> &FakeDirectoryEnvironment {
        &self.directory_environment
    }

    pub fn tmpdir_assignment(&self, name: &str) -> (String, String) {
        let directory = self.scratch_space.directory(&Self::worktree_path(name));
        ("TMPDIR".to_string(), directory.display().to_string())
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
            .with_plan_args(&["--permission-mode".to_string(), "plan".to_string()])
    }

    pub fn agent_with_input() -> Agent {
        Self::agent()
            .with_args(&["--name".to_string(), "{name}".to_string()])
            .with_startup_input(&["/color".to_string()])
            .with_rename_input(&["/rename {name}".to_string()])
    }

    pub fn config_with_input() -> Config {
        Self::config().with_agents(vec![Self::agent_with_input()])
    }

    pub fn config() -> Config {
        Config::defaults(Path::new(Self::WORKTREE_ROOT)).with_agents(vec![Self::agent()])
    }

    pub fn config_with_alt_n() -> Config {
        Self::config().with_key_bindings(Self::alt_n_bindings())
    }

    pub fn alt_n_bindings() -> KeyBindings {
        KeyBindings::default().with_next_session_keys(HotKey::parse_valid(["M-n"]))
    }

    pub fn naming() -> NamingScheme {
        Self::config().naming().clone()
    }

    pub fn pull_request(number: u64, state: PullRequestState) -> PullRequest {
        PullRequest::new(number, state, Self::NOW)
    }

    pub fn session(name: &str) -> Session {
        Self::session_at(name, Self::REPO, Self::CREATED_AT)
    }

    pub fn session_on_branch(name: &str, branch: &str) -> Session {
        let session = Self::session(name);
        let tmux = TmuxAddress::new(session.tmux_name(), session.socket_path());
        session.rekeyed(
            session.name().clone(),
            session.metadata().with_branch(branch),
            tmux,
        )
    }

    pub fn summary_in(name: &str, repo_path: &str, state: SessionState) -> SessionSummary {
        let session = Self::session_at(name, repo_path, Self::CREATED_AT);
        SessionSummary::managed(session, state, false)
    }

    pub fn session_with_agent(name: &str, agent: &str) -> Session {
        let session = Self::session(name);
        Self::rebuilt(&session, Self::recorded_worktree(&session), agent)
    }

    pub fn session_based_on(name: &str, base_branch: &str) -> Session {
        let session = Self::session(name);
        let worktree = Self::recorded_worktree(&session).based_on(BaseBranch::new(base_branch));
        Self::rebuilt(&session, worktree, session.agent())
    }

    fn recorded_worktree(session: &Session) -> SessionWorktree {
        SessionWorktree::new(
            session.worktree_path(),
            session.branch(),
            session.base_commit(),
        )
    }

    fn rebuilt(session: &Session, worktree: SessionWorktree, agent: &str) -> Session {
        let repo = SessionRepo::new(session.repo_name().clone(), session.repo_path());
        let metadata = SessionMetadata::new(repo, worktree, agent);
        let tmux = TmuxAddress::new(session.tmux_name(), session.socket_path());
        session.rekeyed(session.name().clone(), metadata, tmux)
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

    pub fn merging(name: &str, outcome: MergeOutcome, multiplexer: FakeMultiplexer) -> Self {
        let session = Self::session(name);
        Self::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_workspace(Self::workspace_for(&[&session]).with_merge_outcome(outcome))
            .with_host(Self::host_for(&[&session]))
            .with_multiplexer(multiplexer.with_live(Self::waiting(&session)))
    }

    pub fn refusing_typing(name: &str) -> FakeMultiplexer {
        let not_ready = MultiplexerError::AgentNotReady {
            tmux_name: Self::session(name).tmux_name().to_string(),
        };
        FakeMultiplexer::new().failing("type_if_ready", not_ready)
    }

    pub fn not_allowed_envrc(name: &str) -> FakeDirectoryEnvironment {
        FakeDirectoryEnvironment::default().with_envrc(&Self::worktree_path(name), Self::ENVRC)
    }

    pub fn allowed_envrc(name: &str) -> FakeDirectoryEnvironment {
        FakeDirectoryEnvironment::default()
            .with_allowed_envrc(&Self::worktree_path(name), Self::ENVRC)
    }

    pub fn with_worktree_agent(session: &Session) -> FakeHostProbe {
        Self::with_worktree_process(session)
            .serving(Self::WORKTREE_PROCESS, Path::new(FakeMultiplexer::SOCKET))
    }

    pub fn with_worktree_process(session: &Session) -> FakeHostProbe {
        Self::host_for(&[session]).with_process(
            Self::WORKTREE_PROCESS,
            "claude",
            session.worktree_path(),
            Self::WORKTREE_PROCESS,
        )
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
            .with_directory(Path::new(Self::DATA_DIR))
            .with_directory(Path::new(Self::STATE_DIRECTORY));
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

    pub fn ports(&self) -> ServicePorts {
        ServicePorts::new(
            StatePorts::new(
                self.registry.clone(),
                self.config_source.clone(),
                self.clock.clone(),
                self.scratch_space.clone(),
            ),
            RepoPorts::new(
                self.workspace.clone(),
                self.scanner.clone(),
                self.host.clone(),
                self.pull_requests.clone(),
                self.directory_environment.clone(),
            )
            .with_diff_workspace(self.measuring_workspace()),
            TmuxPorts::new(self.multiplexer.clone()),
        )
    }

    fn measuring_workspace(&self) -> Arc<FakeWorkspace> {
        let diff_workspace = self.diff_workspace.as_ref().unwrap_or(&self.workspace);
        Arc::clone(diff_workspace)
    }

    fn context(&self) -> ServiceContext {
        let config = self.config_source.config();
        let loaded = match &self.config_error {
            Some(error) => LoadedConfig::failed(config, error.clone()),
            None => LoadedConfig::valid(config),
        };
        ServiceContext::new(loaded, &self.launch_directory)
            .with_diagnostics(self.diagnostics.clone())
            .with_home(self.home.as_deref())
            .with_shell(&self.shell)
    }
}
