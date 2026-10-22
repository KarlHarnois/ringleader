mod adapters;
mod application;
mod cli;
mod domain;
#[cfg(test)]
mod test_support;
mod ui;

use std::convert;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::iter::once;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use log::LevelFilter;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use adapters::agent_launch::agent_handoff::AgentHandoff;
use adapters::agent_launch::agent_launcher::{AgentLauncher, ProcessExec};
use adapters::agent_launch::agent_probe::AgentProbe;
use adapters::agent_launch::built_in_skill::BuiltInSkill;
use adapters::agent_launch::launcher_command::LauncherCommand;
use adapters::agent_launch::ringleader_shim::RingleaderShim;
use adapters::claude::claude_usage_meter::ClaudeUsageMeter;
use adapters::config_file::toml_config_source::TomlConfigSource;
use adapters::direnv::direnv_directory_environment::DirenvDirectoryEnvironment;
use adapters::filesystem::directory_repo_scanner::DirectoryRepoScanner;
use adapters::filesystem::directory_scratch_space::DirectoryScratchSpace;
use adapters::filesystem::file_lock::FileLock;
use adapters::filesystem::json_fold_store::JsonFoldStore;
use adapters::filesystem::json_session_registry::JsonSessionRegistry;
use adapters::gh::gh_pull_requests::GhPullRequests;
use adapters::gh::null_pull_requests::NullPullRequests;
use adapters::git::git_runner::GitRunner;
use adapters::host::app_log::AppLog;
use adapters::host::process_environment::{ProcessEnvironment, ProgramPaths};
use adapters::host::process_table::ProcessTable;
use adapters::host::signal_hook_interrupts::SignalHookInterrupts;
use adapters::host::system_clock::SystemClock;
use adapters::host::system_host_probe::SystemHostProbe;
use adapters::host::xdg_paths::XdgPaths;
use adapters::ollama::ollama_cloud_quota::OllamaCloudQuota;
use adapters::opencode::opencode_usage_meter::OpencodeUsageMeter;
use adapters::sqlite::sqlite_opencode_database::SqliteOpencodeDatabase;
use adapters::terminal_emulator::system_terminal_emulator::SystemTerminalEmulator;
use adapters::tmux::server_files::ServerFiles;
use adapters::tmux::tmux_command::TmuxCommand;
use adapters::tmux::tmux_finder_popup::TmuxFinderPopup;
use adapters::tmux::tmux_multiplexer::TmuxMultiplexer;
use adapters::web_browser::xdg_web_browser::XdgWebBrowser;
use adapters::{BoundedCommand, Deadlines, GitWorkspace, HostDesktop, HostMounts};
use application::ports::{ConfigSource, HostProbe, Lock, LockError, PullRequests, UsageMeter};
use application::{
    AgentPorts, DesktopPorts, DiagnosticPaths, LoadedConfig, RepoPorts, ServiceContext,
    ServiceError, ServicePorts, SessionService, StatePorts, TmuxPorts,
};
use cli::{Cli, CliExit, CliOutput, CliSession, EntryPoints};
use ui::{
    App, CrosstermInput, CrosstermScreenMode, EventLoop, FinderPopupError, SessionFinderPopup,
    TerminalGuard,
};

fn main() -> ExitCode {
    CompositionRoot::capture().run()
}

struct CompositionRoot {
    environment: ProcessEnvironment,
    programs: ProgramPaths,
    paths: XdgPaths,
    server: ServerFiles,
    launch_directory: PathBuf,
    executable: PathBuf,
    processes: ProcessTable,
}

#[expect(
    clippy::disallowed_methods,
    reason = "the composition root reads the process inputs and injects them"
)]
impl CompositionRoot {
    fn capture() -> Self {
        let snapshot = ProcessEnvironment::capture();
        let (programs, environment) = snapshot.split_program_paths();
        let paths = XdgPaths::from_environment(&environment);
        Self {
            server: Self::server_files(&paths),
            paths,
            launch_directory: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            executable: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("ringleader")),
            environment,
            programs,
            processes: ProcessTable::new(),
        }
    }

    fn server_files(paths: &XdgPaths) -> ServerFiles {
        ServerFiles::new(&paths.tmux_socket(), &paths.tmux_config())
    }

    fn run(self) -> ExitCode {
        Cli::parse_arguments(std::env::args_os())
            .map_or_else(convert::identity, |cli| cli.enter(&self))
    }

    fn run_cli(&self, cli: &Cli) -> ExitCode {
        let state_directory = self.paths.state_directory();
        let state_ready = fs::create_dir_all(&state_directory);
        if let (Err(error), true) = (&state_ready, cli.is_mutating()) {
            return Self::fail(&format!(
                "cannot create {}: {error}",
                state_directory.display()
            ));
        }
        let log = match &state_ready {
            Ok(()) => self.start_log(cli),
            Err(error) => Err(AppLog::disabled(
                &AppLog::path_in(&state_directory),
                &error.to_string(),
            )),
        };
        self.handoff().delete_stale();
        let signals = match cli.is_mutating() {
            true => SignalHookInterrupts::register()
                .unwrap_or_else(|_| SignalHookInterrupts::unregistered()),
            false => SignalHookInterrupts::unregistered(),
        };
        let mut session = CliSession::new(CliOutput::standard(), Box::new(signals));
        if let Err(message) = &log {
            session.output().error(message);
        }
        let mut lock = FileLock::new(&state_directory.join("lock"));
        if let Err(exit) = cli.take_write_lock(&mut lock, &mut session) {
            return Self::exit_with(exit);
        }
        if let (Ok(log), true) = (&log, cli.is_mutating()) {
            log.truncate_if_large();
        }
        let config_source = self.config_source(cli);
        if cli.is_mutating()
            && let Some(warning) = Self::write_template_if_missing(&config_source)
        {
            session.output().error(&warning);
        }
        let loaded = Self::load(&config_source);
        let mut service = self.service(config_source, loaded);
        Self::exit_with(cli.execute(&mut service, &mut session, &mut lock))
    }

    fn start_log(&self, cli: &Cli) -> Result<AppLog, String> {
        let log = AppLog::open(&self.paths.state_directory())?;
        let level = cli.log_level().and_then(|level| level.parse().ok());
        log.start(level.unwrap_or(LevelFilter::Info))?;
        Ok(log)
    }

    fn exit_with(exit: CliExit) -> ExitCode {
        if let CliExit::Interrupted { signal, .. } = exit {
            let _ = signal_hook::low_level::emulate_default_handler(signal);
        }
        ExitCode::from(exit.code())
    }

    fn exec_agent(&self, handoff: &Path) -> ExitCode {
        let reentry = LauncherCommand::new(
            self.launcher_prefix(),
            AgentHandoff::beside(handoff, &self.environment, self.processes.clone()),
        );
        let mut launcher = AgentLauncher::new(&self.environment, Box::new(ProcessExec), reentry);
        Self::fail(&launcher.launch(handoff).to_string())
    }

    fn run_tui(&self, cli: &Cli) -> ExitCode {
        let state_directory = self.paths.state_directory();
        let ran = Self::create_state_directory(&state_directory)
            .and_then(|()| self.run_tui_in(cli, &state_directory));
        ran.err().unwrap_or(ExitCode::SUCCESS)
    }

    fn create_state_directory(state_directory: &Path) -> Result<(), ExitCode> {
        fs::create_dir_all(state_directory).map_err(|error| {
            Self::fail(&format!(
                "cannot create {}: {error}",
                state_directory.display()
            ))
        })
    }

    fn run_tui_in(&self, cli: &Cli, state_directory: &Path) -> Result<(), ExitCode> {
        let log = self.start_log(cli);
        self.handoff().delete_stale();
        let mut instance = Self::held_tui_lock(state_directory)?;
        Self::truncate_log(&log);
        self.started_app(cli, state_directory, log.err())
            .and_then(Self::run_app)
            .map(|(app, signals)| Self::wait_for_startup_input(app, signals, &mut instance))
    }

    fn held_tui_lock(state_directory: &Path) -> Result<FileLock, ExitCode> {
        let mut instance = FileLock::new(&state_directory.join("tui.lock"));
        instance
            .try_acquire()
            .map(|()| instance)
            .map_err(|error| Self::lock_failure(&error))
    }

    fn truncate_log(log: &Result<AppLog, String>) {
        if let Ok(log) = log {
            log.truncate_if_large();
        }
    }

    fn started_app(
        &self,
        cli: &Cli,
        state_directory: &Path,
        log_problem: Option<String>,
    ) -> Result<App, ExitCode> {
        let config_source = self.config_source(cli);
        let template_warning = Self::write_template_if_missing(&config_source);
        let loaded = Self::load(&config_source);
        let startup_message = loaded
            .error()
            .map(|error| format!("config error, running read-only: {error}"))
            .or(template_warning);
        let mut app = App::new(
            self.service(config_source, loaded),
            Box::new(FileLock::new(&state_directory.join("lock"))),
            Box::new(io::stdout()),
            Box::new(JsonFoldStore::new(&state_directory.join("folds.json"))),
            Instant::now(),
        );
        let process_warnings = [log_problem, startup_message]
            .into_iter()
            .flatten()
            .collect();
        app.start_up(process_warnings)
            .map(|()| app)
            .map_err(|error| Self::refuse(&error))
    }

    fn run_app(mut app: App) -> Result<(App, SignalHookInterrupts), ExitCode> {
        let signals =
            SignalHookInterrupts::register().map_err(|error| Self::fail(&error.to_string()))?;
        Self::run_event_loop(&mut app, signals.clone())
            .map(|()| (app, signals))
            .map_err(|error| Self::fail(&error.to_string()))
    }

    fn wait_for_startup_input(app: App, signals: SignalHookInterrupts, instance: &mut FileLock) {
        let mut session = CliSession::new(CliOutput::standard(), Box::new(signals));
        session.wait_for_startup_input(&app.into_service(), instance);
    }

    fn run_event_loop(app: &mut App, signals: SignalHookInterrupts) -> io::Result<()> {
        let screen = TerminalGuard::enter(Box::new(CrosstermScreenMode))?;
        screen.state().install_panic_hook();
        let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        Self::clear_alternate_screen_leftovers(&mut terminal)?;
        EventLoop::new(terminal, CrosstermInput, Box::new(signals), screen.state()).run(app)
    }

    fn clear_alternate_screen_leftovers(
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> io::Result<()> {
        terminal.clear()
    }

    fn config_source(&self, cli: &Cli) -> TomlConfigSource {
        let path = cli
            .config_path()
            .map(Path::to_path_buf)
            .or_else(|| {
                self.environment
                    .get(TomlConfigSource::PATH_VARIABLE)
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| self.paths.config_file());
        TomlConfigSource::new(&path, &self.paths.worktree_root(), self.paths.home())
            .resolved_from(&self.launch_directory)
    }

    fn write_template_if_missing(source: &TomlConfigSource) -> Option<String> {
        if source.exists() {
            return None;
        }
        let error = source.write_first_run_template().err()?;
        Some(format!(
            "could not write the first-run config, using the defaults: {error}"
        ))
    }

    fn load(source: &TomlConfigSource) -> LoadedConfig {
        match source.load() {
            Ok(config) => LoadedConfig::valid(config),
            Err(error) => LoadedConfig::failed(source.defaults(), error),
        }
    }

    fn service(&self, config_source: TomlConfigSource, loaded: LoadedConfig) -> SessionService {
        let env_names = loaded.config().agent_env_union();
        let tmux = self.tmux(&env_names);
        let bounded = BoundedCommand::new(Deadlines::standard());
        let config_path = config_source.path();
        let state = self.state_ports(config_source);
        let host = Arc::new(SystemHostProbe::new(
            &self.environment,
            self.programs.tmux(),
            self.processes.clone(),
        ));
        let pull_requests = Self::pull_requests(&host, bounded);
        let usage_meters = self.usage_meters(host.as_ref(), bounded);
        let directory_environment =
            DirenvDirectoryEnvironment::when_installed(host.as_ref(), &self.environment, bounded);
        let desktop = self.desktop_ports(host.as_ref(), bounded);
        let repos = RepoPorts::new(
            Arc::new(GitWorkspace::new(self.programs.git(), bounded)),
            Arc::new(DirectoryRepoScanner::new(
                GitRunner::new(self.programs.git(), bounded),
                DirectoryRepoScanner::VISIT_CAP,
                Box::new(HostMounts::of_this_process()),
            )),
            host,
            pull_requests,
            directory_environment,
        )
        .with_diff_workspace(Arc::new(GitWorkspace::new(
            self.programs.git(),
            BoundedCommand::new(Deadlines::diff_poll()),
        )));
        let tmux_ports = TmuxPorts::new(Arc::new(TmuxMultiplexer::new(
            tmux,
            self.launcher(&config_path),
            self.processes.clone(),
        )));
        let context = ServiceContext::new(loaded, &self.launch_directory)
            .with_home(self.paths.home())
            .with_diagnostics(self.diagnostics())
            .with_shell(&self.environment.shell());
        SessionService::new(
            ServicePorts::new(
                state,
                repos,
                tmux_ports,
                desktop,
                AgentPorts::new(usage_meters),
            ),
            context,
        )
    }

    fn state_ports(&self, config_source: TomlConfigSource) -> StatePorts {
        let clock = Arc::new(SystemClock);
        let registry = JsonSessionRegistry::new(
            &self.paths.state_directory().join("sessions.json"),
            clock.clone(),
        );
        StatePorts::new(
            Arc::new(registry),
            Arc::new(config_source),
            clock,
            Arc::new(DirectoryScratchSpace::new(
                &self.paths.scratch_root(),
                Box::new(HostMounts::of_this_process()),
            )),
        )
    }

    fn usage_meters(
        &self,
        host: &SystemHostProbe,
        bounded: BoundedCommand,
    ) -> Vec<Arc<dyn UsageMeter>> {
        let probe = Arc::new(AgentProbe::new(
            &self.environment,
            &self.paths.usage_directory(),
            bounded,
        ));
        let quotas = vec![OllamaCloudQuota::when_installed(
            host,
            &self.environment,
            bounded,
        )];
        let database = SqliteOpencodeDatabase::when_installed(host, &self.environment, bounded);
        vec![
            Arc::new(ClaudeUsageMeter::new(probe.clone())),
            Arc::new(OpencodeUsageMeter::new(probe, database, quotas)),
        ]
    }

    fn desktop_ports(&self, host: &SystemHostProbe, bounded: BoundedCommand) -> DesktopPorts {
        let desktop = HostDesktop::current();
        DesktopPorts::new(
            SystemTerminalEmulator::detect(&desktop, host, &self.environment, bounded),
            XdgWebBrowser::detect(&desktop, host, &self.environment),
        )
    }

    fn pull_requests(host: &SystemHostProbe, bounded: BoundedCommand) -> Arc<dyn PullRequests> {
        match host.resolve_command("gh") {
            Some(program) => Arc::new(GhPullRequests::new(program.as_os_str(), bounded)),
            None => Arc::new(NullPullRequests),
        }
    }

    fn diagnostics(&self) -> DiagnosticPaths {
        let paths = DiagnosticPaths::new(
            &self.programs.tmux().to_string_lossy(),
            &self.programs.git().to_string_lossy(),
            &self.paths.state_directory(),
        )
        .with_tmux_server(self.server.socket(), self.server.config())
        .with_log_file(&AppLog::path_in(&self.paths.state_directory()));
        match self.paths.has_runtime_directory() {
            true => paths,
            false => paths.with_socket_in_state_directory(),
        }
    }

    fn tmux(&self, env_names: &[String]) -> TmuxCommand {
        TmuxCommand::new(
            self.programs.tmux(),
            &self.server,
            &self.environment,
            &self.finder_popup(),
        )
        .removing_env_names(env_names)
    }

    fn launcher(&self, config_path: &Path) -> LauncherCommand {
        let handoff = self
            .handoff()
            .with_built_in_skill(BuiltInSkill::new(&self.paths.plugin_root()))
            .with_shim(RingleaderShim::new(
                &self.paths.shim_root(),
                &self.executable,
            ))
            .with_config_path(config_path);
        LauncherCommand::new(self.launcher_prefix(), handoff)
    }

    fn launcher_prefix(&self) -> Vec<OsString> {
        vec![
            self.executable.clone().into_os_string(),
            OsString::from("__exec-agent"),
        ]
    }

    fn finder_popup(&self) -> TmuxFinderPopup {
        let finder_prefix = once(self.executable.clone().into_os_string())
            .chain(Cli::finder_arguments())
            .collect();
        TmuxFinderPopup::new(finder_prefix, &self.programs.assignments())
    }

    fn show_finder(popup: SessionFinderPopup) -> Result<(), FinderPopupError> {
        let screen = TerminalGuard::enter(Box::new(CrosstermScreenMode))?;
        screen.state().install_panic_hook();
        popup.run(CrosstermBackend::new(io::stdout()), CrosstermInput)
    }

    fn handoff(&self) -> AgentHandoff {
        AgentHandoff::new(
            &self.paths.handoff_directory(),
            &self.environment,
            self.processes.clone(),
        )
    }

    fn lock_failure(error: &LockError) -> ExitCode {
        let _ = writeln!(io::stderr(), "ringleader: {error}");
        ExitCode::from(CliExit::lock_error_code(error))
    }

    fn refuse(error: &ServiceError) -> ExitCode {
        let _ = writeln!(io::stderr(), "ringleader: {error}");
        ExitCode::from(CliExit::error_code(error))
    }

    fn fail(message: &str) -> ExitCode {
        let _ = writeln!(io::stderr(), "ringleader: {message}");
        ExitCode::from(CliExit::FAILURE)
    }
}

impl EntryPoints for CompositionRoot {
    fn launch_agent(&self, handoff: &Path) -> ExitCode {
        self.exec_agent(handoff)
    }

    fn socket_problem(&self) -> Option<String> {
        self.diagnostics().socket_problem()
    }

    fn refuse_to_start(&self, problem: &str) -> ExitCode {
        Self::fail(problem)
    }

    fn find_session(&self, cli: &Cli, client: &str) -> ExitCode {
        let _log = self.start_log(cli);
        let config_source = self.config_source(cli);
        let loaded = Self::load(&config_source);
        let popup = SessionFinderPopup::new(self.service(config_source, loaded), client);
        Self::show_finder(popup).map_or_else(
            |error| Self::fail(&error.to_string()),
            |()| ExitCode::SUCCESS,
        )
    }

    fn open_tui(&self, cli: &Cli) -> ExitCode {
        self.run_tui(cli)
    }

    fn run_command(&self, cli: &Cli) -> ExitCode {
        self.run_cli(cli)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::ffi::OsStr;
    use std::fs;
    use std::path::{Path, PathBuf};

    struct LayerImports {
        sources: Vec<(PathBuf, String)>,
    }

    impl LayerImports {
        const SOURCE_DIRECTORY: &'static str = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        const TEST_SUPPORT: &'static str = "test_support";
        const OUTSIDE_THE_CORE: [&'static str; 4] =
            ["crossterm", "ratatui", "std::process", "std::fs"];

        fn of(layers: &[&str]) -> Self {
            let sources = layers
                .iter()
                .flat_map(|layer| Self::layer_files(layer))
                .map(|path| {
                    let text = fs::read_to_string(&path).expect("a readable source file");
                    (path, text)
                })
                .collect();
            Self { sources }
        }

        fn from_text(text: &str) -> Self {
            Self {
                sources: vec![(PathBuf::from("example.rs"), text.to_string())],
            }
        }

        fn layers_outside(&self, allowed: &[&str]) -> BTreeSet<String> {
            self.references("crate")
                .filter(|(_, module)| {
                    module != Self::TEST_SUPPORT && !allowed.contains(&module.as_str())
                })
                .map(|(file, module)| format!("{} uses crate::{module}", file.display()))
                .collect()
        }

        fn uses_of(&self, paths: &[&str]) -> BTreeSet<String> {
            paths
                .iter()
                .flat_map(|path| self.uses_of_path(path))
                .collect()
        }

        fn uses_of_path(&self, path: &str) -> BTreeSet<String> {
            let (root, module) = path
                .split_once("::")
                .map_or((path, None), |(root, module)| (root, Some(module)));
            self.references(root)
                .filter(|(_, found)| module.is_none_or(|module| module == found))
                .map(|(file, _)| format!("{} uses {path}", file.display()))
                .collect()
        }

        fn layer_files(layer: &str) -> Vec<PathBuf> {
            let source = Path::new(Self::SOURCE_DIRECTORY);
            let mut files = Self::rust_files_under(&source.join(layer));
            let module_file = source.join(format!("{layer}.rs"));
            if module_file.is_file() {
                files.push(module_file);
            }
            files
        }

        fn rust_files_under(directory: &Path) -> Vec<PathBuf> {
            let mut files = Vec::new();
            for entry in fs::read_dir(directory).expect("a readable layer directory") {
                let path = entry.expect("a readable directory entry").path();
                if path.is_dir() {
                    files.extend(Self::rust_files_under(&path));
                } else if path.extension() == Some(OsStr::new("rs")) {
                    files.push(path);
                }
            }
            files
        }

        fn references(&self, root: &str) -> impl Iterator<Item = (&Path, String)> {
            let prefix = format!("{root}::");
            self.sources.iter().flat_map(move |(path, text)| {
                let file = path.strip_prefix(Self::SOURCE_DIRECTORY).unwrap_or(path);
                Self::modules_after(text, &prefix)
                    .into_iter()
                    .map(move |module| (file, module))
            })
        }

        fn modules_after(text: &str, prefix: &str) -> Vec<String> {
            text.match_indices(prefix)
                .filter(|(index, _)| !Self::continues_a_name(text, *index))
                .flat_map(|(index, _)| Self::first_segments(&text[index + prefix.len()..]))
                .filter(|module| !module.is_empty())
                .collect()
        }

        fn continues_a_name(text: &str, index: usize) -> bool {
            text[..index]
                .chars()
                .next_back()
                .is_some_and(|character| character.is_alphanumeric() || character == '_')
        }

        fn first_segments(path: &str) -> Vec<String> {
            match path.strip_prefix('{') {
                Some(group) => Self::group_items(group)
                    .into_iter()
                    .map(Self::leading_name)
                    .collect(),
                None => vec![Self::leading_name(path)],
            }
        }

        fn group_items(group: &str) -> Vec<&str> {
            let mut items = Vec::new();
            let mut depth = 0;
            let mut start = 0;
            for (index, character) in group.char_indices() {
                match character {
                    '{' => depth += 1,
                    '}' if depth == 0 => {
                        items.push(&group[start..index]);
                        break;
                    }
                    '}' => depth -= 1,
                    ',' if depth == 0 => {
                        items.push(&group[start..index]);
                        start = index + 1;
                    }
                    _ => {}
                }
            }
            items
        }

        fn leading_name(path: &str) -> String {
            path.trim_start()
                .chars()
                .take_while(|character| character.is_alphanumeric() || *character == '_')
                .collect()
        }
    }

    #[test]
    fn domain_imports_no_other_layer() {
        let outside = LayerImports::of(&["domain"]).layers_outside(&["domain"]);

        assert_eq!(outside, BTreeSet::new());
    }

    #[test]
    fn application_imports_only_domain() {
        let outside = LayerImports::of(&["application"]).layers_outside(&["application", "domain"]);

        assert_eq!(outside, BTreeSet::new());
    }

    #[test]
    fn adapters_import_neither_ui_nor_cli() {
        let outside =
            LayerImports::of(&["adapters"]).layers_outside(&["adapters", "application", "domain"]);

        assert_eq!(outside, BTreeSet::new());
    }

    #[test]
    fn ui_imports_neither_adapters_nor_cli() {
        let outside = LayerImports::of(&["ui"]).layers_outside(&["ui", "application", "domain"]);

        assert_eq!(outside, BTreeSet::new());
    }

    #[test]
    fn cli_imports_neither_adapters_nor_ui() {
        let outside = LayerImports::of(&["cli"]).layers_outside(&["cli", "application", "domain"]);

        assert_eq!(outside, BTreeSet::new());
    }

    #[test]
    fn the_core_never_uses_crossterm_ratatui_std_process_or_std_fs() {
        let uses =
            LayerImports::of(&["domain", "application"]).uses_of(&LayerImports::OUTSIDE_THE_CORE);

        assert_eq!(uses, BTreeSet::new());
    }

    #[test]
    fn only_main_builds_the_production_process_table_so_no_test_scans_the_whole_host() {
        let layers = [
            "domain",
            "application",
            "adapters",
            "ui",
            "cli",
            "test_support",
        ];

        let uses = LayerImports::of(&layers).uses_of(&["ProcessTable::new"]);

        assert_eq!(uses, BTreeSet::new());
    }

    #[test]
    fn no_layer_fixes_a_color_so_the_terminal_theme_decides_every_color() {
        let layers = ["domain", "application", "adapters", "ui", "cli"];

        let uses = LayerImports::of(&layers).uses_of(&[
            "Color::Rgb",
            "Color::Indexed",
            "Color::from_u32",
            "Color::from_hsl",
            "Color::from_hsluv",
            "Color::from",
            "Color::from_str",
        ]);

        assert_eq!(uses, BTreeSet::new());
    }

    #[test]
    fn a_grouped_import_counts_each_module_it_names() {
        let imports = LayerImports::from_text(
            "use crate::{domain::Session, adapters::{GitWorkspace, TmuxMultiplexer}};",
        );

        assert_eq!(
            imports.layers_outside(&["domain"]),
            BTreeSet::from(["example.rs uses crate::adapters".to_string()])
        );
    }

    #[test]
    fn a_name_that_only_ends_in_a_root_is_not_that_root() {
        let imports = LayerImports::from_text("use mystd::fs::File;");

        assert_eq!(imports.uses_of(&["std::fs"]), BTreeSet::new());
    }
}
