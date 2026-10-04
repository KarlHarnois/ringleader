mod adapters;
mod application;
mod cli;
mod domain;
#[cfg(test)]
mod test_support;
mod ui;

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use log::LevelFilter;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use adapters::agent_handoff::AgentHandoff;
use adapters::agent_launcher::{AgentLauncher, LauncherCommand, ProcessExec};
use adapters::app_log::AppLog;
use adapters::caller_process::CallerProcess;
use adapters::directory_repo_scanner::DirectoryRepoScanner;
use adapters::file_lock::FileLock;
use adapters::git_runner::GitRunner;
use adapters::json_session_registry::JsonSessionRegistry;
use adapters::process_environment::{ProcessEnvironment, ProgramPaths};
use adapters::signal_flags::SignalFlags;
use adapters::system_clock::SystemClock;
use adapters::system_host_probe::SystemHostProbe;
use adapters::tmux_command::TmuxCommand;
use adapters::tmux_multiplexer::TmuxMultiplexer;
use adapters::tmux_return_binding::TmuxReturnBinding;
use adapters::toml_config_source::TomlConfigSource;
use adapters::xdg_paths::XdgPaths;
use adapters::{BoundedCommand, Deadlines, GitWorkspace};
use application::ports::{ConfigSource, Lock, LockError, ReturnBinding};
use application::{
    CorruptRegistry, DiagnosticPaths, LoadedConfig, RepoPorts, ServiceContext, ServicePorts,
    SessionService, StatePorts, TmuxPorts,
};
use cli::{Cli, CliExit, CliOutput, CliSession};
use ui::app::{BindingGuard, ExitNotice};
use ui::{App, CrosstermInput, CrosstermScreenMode, EventLoop, TerminalGuard};

fn main() -> ExitCode {
    CompositionRoot::capture().run()
}

struct CompositionRoot {
    environment: ProcessEnvironment,
    programs: ProgramPaths,
    paths: XdgPaths,
    launch_directory: PathBuf,
}

impl CompositionRoot {
    const FAILURE: u8 = 1;
    const CONFLICT: u8 = 3;

    fn capture() -> Self {
        let snapshot = ProcessEnvironment::capture();
        let (programs, environment) = snapshot.split_program_paths();
        Self {
            paths: XdgPaths::from_environment(&environment),
            launch_directory: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            environment,
            programs,
        }
    }

    fn run(self) -> ExitCode {
        let cli = match Cli::parse_arguments(std::env::args_os()) {
            Ok(cli) => cli,
            Err(code) => return code,
        };
        if let Some(handoff) = cli.exec_agent_handoff() {
            return self.exec_agent(handoff);
        }
        if cli.runs_tui() {
            return self.run_tui(&cli);
        }
        self.run_cli(&cli)
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
                &state_directory.join(AppLog::FILE_NAME),
                &error.to_string(),
            )),
        };
        self.handoff().delete_stale();
        let signals = match cli.is_mutating() {
            true => SignalFlags::register().unwrap_or_else(|_| SignalFlags::unregistered()),
            false => SignalFlags::unregistered(),
        };
        let mut session = CliSession::new(CliOutput::standard(), signals);
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
        let inside_tmux = self.tmux(&[]).is_inside_client(&CallerProcess::read());
        let binding = self.return_binding(&loaded, inside_tmux);
        let mut service = self.service(config_source, loaded, inside_tmux, binding);
        Self::exit_with(cli.execute(&mut service, &mut session))
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
        let mut launcher = AgentLauncher::new(&self.environment, Box::new(ProcessExec));
        Self::fail(&launcher.launch(handoff).to_string())
    }

    fn run_tui(&self, cli: &Cli) -> ExitCode {
        let state_directory = self.paths.state_directory();
        if let Err(error) = fs::create_dir_all(&state_directory) {
            return Self::fail(&format!(
                "cannot create {}: {error}",
                state_directory.display()
            ));
        }
        let log = self.start_log(cli);
        self.handoff().delete_stale();
        let mut lock = FileLock::new(&state_directory.join("lock"));
        if let Err(error) = lock.acquire_lifetime() {
            return Self::lock_failure(&error);
        }
        if let Ok(log) = &log {
            log.truncate_if_large();
        }
        let config_source = self.config_source(cli);
        let template_warning = Self::write_template_if_missing(&config_source);
        let loaded = Self::load(&config_source);
        let startup_message = loaded
            .error()
            .map(|error| format!("config error, running read-only: {error}"))
            .or(template_warning);
        let inside_tmux = self.tmux(&[]).is_inside_client(&CallerProcess::read());
        let binding = self.return_binding(&loaded, inside_tmux);
        let mut service = self.service(config_source, loaded, inside_tmux, binding.clone());
        let registry_warning = match service.load_registry(CorruptRegistry::MoveAside) {
            Ok(warning) => warning,
            Err(error) => return Self::fail(&error.to_string()),
        };
        let notice = ExitNotice::new(Box::new(io::stderr()));
        let binding_guard = BindingGuard::new(binding, notice.handle());
        let warnings: Vec<String> = [
            log.err(),
            registry_warning,
            startup_message,
            binding_guard.repair_stale(),
        ]
        .into_iter()
        .flatten()
        .collect();
        let app = App::new(service, Box::new(io::stdout()), Instant::now())
            .with_binding_guard(binding_guard)
            .with_message((!warnings.is_empty()).then(|| warnings.join("; ")));
        let outcome = Self::run_event_loop(app);
        drop(notice);
        match outcome {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => Self::fail(&error.to_string()),
        }
    }

    fn run_event_loop(app: App) -> io::Result<()> {
        let signals = SignalFlags::register()?;
        let screen = TerminalGuard::enter(Box::new(CrosstermScreenMode))?;
        screen.state().install_panic_hook();
        let mut app_inside_the_screen = app;
        let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        terminal.clear()?;
        EventLoop::new(terminal, CrosstermInput, signals, screen.state())
            .run(&mut app_inside_the_screen)
    }

    fn config_source(&self, cli: &Cli) -> TomlConfigSource {
        let path = cli
            .config_path()
            .map(Path::to_path_buf)
            .or_else(|| self.environment.get("RINGLEADER_CONFIG").map(PathBuf::from))
            .unwrap_or_else(|| self.paths.config_file());
        TomlConfigSource::new(&path, &self.paths.worktree_root(), self.paths.home())
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

    fn return_binding(&self, loaded: &LoadedConfig, inside_tmux: bool) -> Arc<dyn ReturnBinding> {
        let env_names = loaded.config().agent_env_union();
        Arc::new(TmuxReturnBinding::new(
            self.tmux(&env_names),
            &self.paths.state_directory().join("bindings.json"),
            inside_tmux,
            loaded.config().naming().tmux_prefix(),
        ))
    }

    fn service(
        &self,
        config_source: TomlConfigSource,
        loaded: LoadedConfig,
        inside_tmux: bool,
        binding: Arc<dyn ReturnBinding>,
    ) -> SessionService {
        let env_names = loaded.config().agent_env_union();
        let tmux = self.tmux(&env_names);
        let bounded = BoundedCommand::new(Deadlines::standard());
        let clock = Arc::new(SystemClock);
        let state_directory = self.paths.state_directory();
        let state = StatePorts::new(
            Arc::new(JsonSessionRegistry::new(
                &state_directory.join("sessions.json"),
                clock.clone(),
            )),
            Arc::new(config_source),
            clock,
        );
        let repos = RepoPorts::new(
            Arc::new(GitWorkspace::new(self.programs.git(), bounded)),
            Arc::new(DirectoryRepoScanner::new(
                GitRunner::new(self.programs.git(), bounded),
                DirectoryRepoScanner::VISIT_CAP,
                Path::new(DirectoryRepoScanner::MOUNT_TABLE),
            )),
            Arc::new(SystemHostProbe::new(&self.environment)),
        );
        let tmux_ports = TmuxPorts::new(
            Arc::new(TmuxMultiplexer::new(tmux, self.launcher(), inside_tmux)),
            binding,
        );
        let context = ServiceContext::new(loaded, &self.launch_directory)
            .with_inside_tmux(inside_tmux)
            .with_home(self.paths.home())
            .with_diagnostics(DiagnosticPaths::new(
                &self.programs.tmux().to_string_lossy(),
                &self.programs.git().to_string_lossy(),
                &self.paths.state_directory(),
            ))
            .with_shell(&self.shell());
        SessionService::new(ServicePorts::new(state, repos, tmux_ports), context)
    }

    fn tmux(&self, env_names: &[String]) -> TmuxCommand {
        TmuxCommand::new(
            self.programs.tmux(),
            &self.environment,
            BoundedCommand::new(Deadlines::standard()),
        )
        .removing_env_names(env_names)
    }

    fn launcher(&self) -> LauncherCommand {
        let executable = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("ringleader"));
        let prefix = vec![executable.into_os_string(), OsString::from("__exec-agent")];
        LauncherCommand::new(prefix, self.handoff())
    }

    fn handoff(&self) -> AgentHandoff {
        AgentHandoff::new(&self.paths.handoff_directory(), &self.environment)
    }

    fn shell(&self) -> String {
        self.environment
            .get("SHELL")
            .map(|shell| shell.to_string_lossy().into_owned())
            .unwrap_or_else(|| "sh".to_string())
    }

    fn lock_failure(error: &LockError) -> ExitCode {
        let code = match error {
            LockError::Held { .. } => Self::CONFLICT,
            LockError::Io { .. } => Self::FAILURE,
        };
        let _ = writeln!(io::stderr(), "ringleader: {error}");
        ExitCode::from(code)
    }

    fn fail(message: &str) -> ExitCode {
        let _ = writeln!(io::stderr(), "ringleader: {message}");
        ExitCode::from(Self::FAILURE)
    }
}
