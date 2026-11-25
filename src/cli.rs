mod agent_commands;
mod agent_list_report;
mod cli_exit;
mod cli_input;
mod cli_output;
mod cli_session;
mod create_reply;
mod delete_report;
mod entry_points;
mod envelope;
mod list_report;
mod prompt_argument;
mod rename_reply;
mod reply;
mod repo_argument;
mod repo_commands;
mod repo_list_report;
mod restart_reply;
mod send_reply;
mod session_commands;
mod stop_reply;
mod version_reply;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use agent_commands::AgentCommands;
pub use cli_exit::CliExit;
pub use cli_input::CliInput;
pub use cli_output::{CliOutput, OutputMode};
pub use cli_session::CliSession;
use cli_session::CommandStop;
pub use entry_points::EntryPoints;
pub use envelope::EnvelopeError;
use prompt_argument::PromptArgument;
pub use reply::{Remark, Reply};
use repo_commands::RepoCommands;
use session_commands::SessionCommands;
pub use version_reply::VersionReply;

use crate::application::diagnostics::doctor_report::DoctorReport;
use crate::application::ports::lock::LockPatience;
use crate::application::ports::{Lock, LockError};
use crate::application::{CorruptRegistry, FailureKind, SessionService};
use crate::domain::BuildVersion;

#[derive(Debug, Parser)]
#[command(
    name = CliOutput::PROGRAM,
    version = BuildVersion::CURRENT.text(),
    about = "A tmux session manager for AI coding agents"
)]
pub struct Cli {
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        value_name = "LEVEL",
        value_parser = ["off", "error", "warn", "info", "debug", "trace"]
    )]
    log_level: Option<String>,
    #[command(subcommand)]
    command: Option<CliCommand>,
}

#[derive(Debug, Subcommand)]
enum CliCommand {
    #[command(about = "List every session with its state and diff")]
    List(ListArgs),
    #[command(about = "Create a session in a new worktree or the main checkout")]
    New(NewArgs),
    #[command(about = "Delete sessions and the worktrees and branches made for them")]
    Delete(DeleteArgs),
    #[command(about = "Attach to a session")]
    Attach(NameArgs),
    #[command(about = "Type a prompt into a running session's agent and submit it")]
    Send(SendArgs),
    #[command(about = "Rename a session and the branch made for it")]
    Rename(RenameArgs),
    #[command(about = "Restart exited or stopped sessions, or with --all every one")]
    Restart(RestartArgs),
    #[command(
        about = "Stop waiting or exited sessions, or with --repo every one in a repo, restart resumes them"
    )]
    Stop(StopArgs),
    #[command(about = "Add, remove, or list repos")]
    Repo {
        #[command(subcommand)]
        action: RepoAction,
    },
    #[command(about = "Manage the configured agents")]
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },
    #[command(about = "Check tmux, git, the config, and every session")]
    Doctor(JsonArgs),
    #[command(about = "Print the version")]
    Version(JsonArgs),
    #[command(name = "__exec-agent", hide = true)]
    ExecAgent {
        #[arg(long, value_name = "PATH")]
        handoff: PathBuf,
    },
    #[command(name = Cli::FIND_SESSION, hide = true)]
    FindSession {
        #[arg(long = Cli::CLIENT_OPTION, value_name = "CLIENT")]
        client: String,
    },
    #[command(name = Cli::SESSION_MENU, hide = true)]
    SessionMenu {
        #[arg(long = Cli::CLIENT_OPTION, value_name = "CLIENT")]
        client: String,
    },
}

#[derive(Debug, Subcommand)]
enum RepoAction {
    #[command(about = "Pin a repo as a [[repos]] entry")]
    Add(RepoAddArgs),
    #[command(about = "Remove a [[repos]] entry")]
    Remove(RepoRemoveArgs),
    #[command(about = "List the repos the picker shows")]
    List(JsonArgs),
}

#[derive(Debug, Subcommand)]
enum AgentAction {
    #[command(about = "List the configured agents")]
    List(JsonArgs),
}

#[derive(Debug, Args)]
struct LockArgs {
    #[arg(long, value_name = "SECONDS")]
    lock_timeout: Option<u64>,
}

#[derive(Debug, Args)]
struct RepoAddArgs {
    path: String,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    base_branch: Option<String>,
    #[command(flatten)]
    lock: LockArgs,
}

#[derive(Debug, Args)]
struct RepoRemoveArgs {
    name: String,
    #[command(flatten)]
    lock: LockArgs,
}

#[derive(Debug, Args)]
struct JsonArgs {
    #[arg(long)]
    json: bool,
}

type ListArgs = JsonArgs;

impl Reply for DoctorReport {
    fn lines(&self) -> Vec<String> {
        DoctorReport::lines(self)
    }

    fn result(&self) -> impl Serialize {
        DoctorReport::findings(self)
    }
}

#[derive(Debug, Args)]
struct NewArgs {
    #[arg(long)]
    name: String,
    #[arg(long)]
    repo: String,
    #[arg(long)]
    agent: Option<String>,
    #[arg(long)]
    base_branch: Option<String>,
    #[arg(long, conflicts_with = "base_branch")]
    main_checkout: bool,
    #[arg(long)]
    plan: bool,
    #[arg(long, conflicts_with = "prompt_file")]
    prompt: Option<String>,
    #[arg(long, value_name = "PATH|-")]
    prompt_file: Option<String>,
    #[command(flatten)]
    lock: LockArgs,
    #[command(flatten)]
    json: JsonArgs,
}

#[derive(Debug, Args)]
struct DeleteArgs {
    #[arg(required = true)]
    names: Vec<String>,
    #[arg(long)]
    force: bool,
    #[arg(long)]
    forget: bool,
    #[command(flatten)]
    lock: LockArgs,
}

#[derive(Debug, Args)]
struct NameArgs {
    name: String,
}

#[derive(Debug, Args)]
struct NamesArgs {
    #[arg(required = true)]
    names: Vec<String>,
}

#[derive(Debug, Args)]
#[command(group(
    clap::ArgGroup::new("prompt_source")
        .required(true)
        .multiple(false)
        .args(["prompt", "prompt_file"])
))]
struct SendArgs {
    name: String,
    #[arg(allow_hyphen_values = true)]
    prompt: Option<String>,
    #[arg(long, value_name = "PATH|-")]
    prompt_file: Option<String>,
    #[command(flatten)]
    json: JsonArgs,
}

#[derive(Debug, Args)]
#[command(group(
    clap::ArgGroup::new("restart_target")
        .required(true)
        .multiple(false)
        .args(["names", "all"])
))]
struct RestartArgs {
    names: Vec<String>,
    #[arg(long)]
    all: bool,
    #[command(flatten)]
    lock: LockArgs,
    #[command(flatten)]
    json: JsonArgs,
}

#[derive(Debug, Args)]
#[command(group(
    clap::ArgGroup::new("stop_target")
        .required(true)
        .multiple(false)
        .args(["names", "repo"])
))]
struct StopArgs {
    names: Vec<String>,
    #[arg(long)]
    repo: Option<String>,
    #[command(flatten)]
    lock: LockArgs,
    #[command(flatten)]
    json: JsonArgs,
}

#[derive(Debug, Args)]
struct RenameArgs {
    old: String,
    new: String,
    #[command(flatten)]
    lock: LockArgs,
    #[command(flatten)]
    json: JsonArgs,
}

impl Cli {
    const LOCK_WAIT_NOTE: &'static str = "waiting for another Ringleader to finish a change";
    const FIND_SESSION: &'static str = "__find-session";
    const SESSION_MENU: &'static str = "__session-menu";
    const CLIENT_OPTION: &'static str = "client";

    pub fn parse_arguments<I, A>(arguments: I) -> Result<Self, ExitCode>
    where
        I: IntoIterator<Item = A>,
        A: Into<OsString> + Clone,
    {
        Self::try_parse_from(arguments).map_err(|error| {
            let _ = error.print();
            ExitCode::from(Self::exit_code_for(&error))
        })
    }

    pub fn finder_arguments() -> [OsString; 2] {
        Self::popup_arguments(Self::FIND_SESSION)
    }

    pub fn menu_arguments() -> [OsString; 2] {
        Self::popup_arguments(Self::SESSION_MENU)
    }

    fn popup_arguments(subcommand: &str) -> [OsString; 2] {
        [
            OsString::from(subcommand),
            OsString::from(format!("--{}", Self::CLIENT_OPTION)),
        ]
    }

    pub fn enter(&self, entry_points: &dyn EntryPoints) -> ExitCode {
        if let Some(handoff) = self.exec_agent_handoff() {
            return entry_points.launch_agent(handoff);
        }
        if self.version_args().is_some() {
            return entry_points.print_version(&Self::version_reply(), self.output_mode());
        }
        if let Some(problem) = entry_points.socket_problem()
            && !self.is_doctor()
        {
            return entry_points.refuse_to_start(&problem);
        }
        if let Some(client) = self.finder_client() {
            return entry_points.find_session(self, client);
        }
        if let Some(client) = self.menu_client() {
            return entry_points.show_session_menu(self, client);
        }
        if self.runs_tui() {
            return entry_points.open_tui(self);
        }
        entry_points.run_command(self)
    }

    pub fn config_path(&self) -> Option<&Path> {
        self.config.as_deref()
    }

    pub fn log_level(&self) -> Option<&str> {
        self.log_level.as_deref()
    }

    fn exec_agent_handoff(&self) -> Option<&Path> {
        match &self.command {
            Some(CliCommand::ExecAgent { handoff }) => Some(handoff),
            _ => None,
        }
    }

    fn version_args(&self) -> Option<&JsonArgs> {
        match &self.command {
            Some(CliCommand::Version(args)) => Some(args),
            _ => None,
        }
    }

    fn version_reply() -> VersionReply {
        VersionReply::new(format!(
            "{} {}",
            CliOutput::PROGRAM,
            BuildVersion::CURRENT.text()
        ))
    }

    fn finder_client(&self) -> Option<&str> {
        match &self.command {
            Some(CliCommand::FindSession { client }) => Some(client),
            _ => None,
        }
    }

    fn menu_client(&self) -> Option<&str> {
        match &self.command {
            Some(CliCommand::SessionMenu { client }) => Some(client),
            _ => None,
        }
    }

    fn runs_tui(&self) -> bool {
        self.command.is_none()
    }

    fn is_doctor(&self) -> bool {
        matches!(self.command, Some(CliCommand::Doctor(_)))
    }

    pub fn output_mode(&self) -> OutputMode {
        match self.json_args() {
            Some(args) if args.json => OutputMode::Json,
            _ => OutputMode::Text,
        }
    }

    fn json_args(&self) -> Option<&JsonArgs> {
        match &self.command {
            Some(CliCommand::List(args))
            | Some(CliCommand::Doctor(args))
            | Some(CliCommand::Version(args))
            | Some(CliCommand::Repo {
                action: RepoAction::List(args),
            })
            | Some(CliCommand::Agent {
                action: AgentAction::List(args),
            }) => Some(args),
            Some(CliCommand::New(args)) => Some(&args.json),
            Some(CliCommand::Send(args)) => Some(&args.json),
            Some(CliCommand::Rename(args)) => Some(&args.json),
            Some(CliCommand::Restart(args)) => Some(&args.json),
            Some(CliCommand::Stop(args)) => Some(&args.json),
            _ => None,
        }
    }

    fn lock_args(&self) -> Option<&LockArgs> {
        match &self.command {
            Some(
                CliCommand::New(NewArgs { lock, .. })
                | CliCommand::Delete(DeleteArgs { lock, .. })
                | CliCommand::Rename(RenameArgs { lock, .. })
                | CliCommand::Restart(RestartArgs { lock, .. })
                | CliCommand::Stop(StopArgs { lock, .. })
                | CliCommand::Repo {
                    action:
                        RepoAction::Add(RepoAddArgs { lock, .. })
                        | RepoAction::Remove(RepoRemoveArgs { lock, .. }),
                },
            ) => Some(lock),
            _ => None,
        }
    }

    pub fn is_mutating(&self) -> bool {
        self.lock_args().is_some()
    }

    fn lock_patience(&self) -> LockPatience {
        match self.lock_args().and_then(|args| args.lock_timeout) {
            Some(seconds) => LockPatience::of(Duration::from_secs(seconds)),
            None => LockPatience::DEFAULT,
        }
    }

    pub fn read_prompt(&self, session: &mut CliSession) -> Result<(), CliExit> {
        let Some(argument) = self.prompt_argument() else {
            return Ok(());
        };
        match session.resolve_prompt(&argument) {
            Ok(prompt) => {
                session.store_prompt(prompt);
                Ok(())
            }
            Err(error) => {
                session
                    .output()
                    .print_failure(FailureKind::Failed, &error.to_string());
                Err(CliExit::Code(CliExit::exit_code(FailureKind::Failed)))
            }
        }
    }

    fn prompt_argument(&self) -> Option<PromptArgument<'_>> {
        match &self.command {
            Some(CliCommand::New(args)) => Some(PromptArgument::new(
                args.prompt.as_deref(),
                args.prompt_file.as_deref(),
            )),
            Some(CliCommand::Send(args)) => Some(PromptArgument::new(
                args.prompt.as_deref(),
                args.prompt_file.as_deref(),
            )),
            _ => None,
        }
    }

    pub fn take_write_lock(
        &self,
        lock: &mut dyn Lock,
        session: &mut CliSession,
    ) -> Result<(), CliExit> {
        if !self.is_mutating() {
            return Ok(());
        }
        let patience = self.lock_patience();
        let taken = match lock.try_acquire() {
            Err(LockError::Held { .. }) if patience.allows_a_wait() => {
                Self::wait_for_lock(lock, patience, session)
            }
            attempt => attempt,
        };
        taken.map_err(|error| Self::lock_failure(&error, session))
    }

    fn wait_for_lock(
        lock: &mut dyn Lock,
        patience: LockPatience,
        session: &mut CliSession,
    ) -> Result<(), LockError> {
        session.output().note(Self::LOCK_WAIT_NOTE);
        lock.acquire(patience, &|| session.is_signaled())
    }

    fn lock_failure(error: &LockError, session: &mut CliSession) -> CliExit {
        if let Err(stop) = session.stop_if_signaled() {
            return session.finish(Err(stop));
        }
        let kind = error.failure_kind();
        session.output().print_failure(kind, &error.to_string());
        CliExit::Code(CliExit::exit_code(kind))
    }

    pub fn execute(
        &self,
        service: &mut SessionService,
        session: &mut CliSession,
        lock: &mut dyn Lock,
    ) -> CliExit {
        if let Some(CliCommand::Doctor(_)) = &self.command {
            return match Self::doctor(service, session) {
                Ok(code) => CliExit::Code(code),
                Err(stop) => session.finish(Err(stop)),
            };
        }
        if let Some(error) = service.read_only_reason() {
            session
                .output()
                .print_failure(FailureKind::Failed, &error.to_string());
            return CliExit::Code(CliExit::exit_code(FailureKind::Failed));
        }
        Self::restore_server_socket(service, session);
        let recovery = if self.is_mutating() {
            CorruptRegistry::MoveAside
        } else {
            CorruptRegistry::Report
        };
        match service.load_registry(recovery) {
            Ok(Some(warning)) => session.output().error(&warning),
            Ok(None) => {}
            Err(error) => {
                let kind = error.failure_kind();
                session.output().print_failure(kind, &error.to_string());
                return CliExit::Code(CliExit::exit_code(kind));
            }
        }
        let result = self.dispatch(service, session);
        lock.release();
        session.wait_for_startup_input(service);
        let exit = session.finish(result);
        self.purge_retired_scratch(service, session);
        exit
    }

    fn purge_retired_scratch(&self, service: &SessionService, session: &CliSession) {
        if self.is_mutating() {
            service
                .start_scratch_purge()
                .wait_unless(|| session.is_signaled());
        }
    }

    fn restore_server_socket(service: &SessionService, session: &mut CliSession) {
        match service.restore_server_socket() {
            Ok(Some(note)) => session.output().note(&note),
            Ok(None) => {}
            Err(error) => session.output().error(&error.to_string()),
        }
    }

    fn doctor(service: &SessionService, session: &mut CliSession) -> Result<u8, CommandStop> {
        let report = service.doctor();
        session.output().reply(&report)?;
        Ok(u8::from(report.has_problems()))
    }

    fn dispatch(
        &self,
        service: &mut SessionService,
        session: &mut CliSession,
    ) -> Result<u8, CommandStop> {
        match &self.command {
            Some(CliCommand::Repo { action }) => Self::dispatch_repo(action, service, session),
            Some(CliCommand::Agent { action }) => Self::dispatch_agent(action, service, session),
            Some(CliCommand::Send(args)) => SessionCommands::new(service, session).send(args),
            Some(command) => Self::dispatch_session(command, service, session),
            None => Ok(0),
        }
    }

    fn dispatch_session(
        command: &CliCommand,
        service: &mut SessionService,
        session: &mut CliSession,
    ) -> Result<u8, CommandStop> {
        let mut commands = SessionCommands::new(service, session);
        match command {
            CliCommand::List(_) => commands.list(),
            CliCommand::New(args) => commands.create(args),
            CliCommand::Delete(args) => commands.delete(args),
            CliCommand::Attach(args) => commands.attach(&args.name),
            CliCommand::Rename(args) => commands.rename(args),
            CliCommand::Restart(args) => commands.restart_named_or_all(args),
            CliCommand::Stop(args) => commands.stop_named_or_repo(args),
            CliCommand::Repo { .. }
            | CliCommand::Agent { .. }
            | CliCommand::Send(_)
            | CliCommand::Doctor(_)
            | CliCommand::Version(_)
            | CliCommand::ExecAgent { .. }
            | CliCommand::FindSession { .. }
            | CliCommand::SessionMenu { .. } => Ok(0),
        }
    }

    fn dispatch_repo(
        action: &RepoAction,
        service: &mut SessionService,
        session: &mut CliSession,
    ) -> Result<u8, CommandStop> {
        let mut repos = RepoCommands::new(service, session);
        match action {
            RepoAction::Add(args) => repos.add(args),
            RepoAction::Remove(args) => repos.remove(&args.name),
            RepoAction::List(_) => repos.list(),
        }
    }

    fn dispatch_agent(
        action: &AgentAction,
        service: &SessionService,
        session: &mut CliSession,
    ) -> Result<u8, CommandStop> {
        let mut agents = AgentCommands::new(service, session);
        match action {
            AgentAction::List(_) => agents.list(),
        }
    }

    fn exit_code_for(error: &clap::Error) -> u8 {
        if error.use_stderr() {
            CliExit::FAILURE
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use signal_hook::consts::SIGINT;

    use super::*;
    use crate::test_support::{
        CapturedWriter, CliHarness, Deadline, FakeLock, FakeMultiplexer, FakeScratchSpace, Gate,
        ServiceFixture,
    };

    struct LockFlowFixture;

    impl LockFlowFixture {
        const TYPING_DELAY: Duration = Duration::from_millis(200);
        const LONG_HOLD: Duration = Duration::from_secs(5);

        fn release_typing_later(multiplexer: &Arc<FakeMultiplexer>) {
            let delayed = Arc::clone(multiplexer);
            thread::spawn(move || {
                thread::sleep(Self::TYPING_DELAY);
                delayed.release_typing();
            });
        }

        fn wait_for(log: &Arc<Mutex<Vec<String>>>, event: &str, limit: Duration) -> bool {
            Deadline::after(limit)
                .wait_until(|| log.lock().unwrap().iter().any(|item| item == event))
        }

        fn recorded_before(log: &Arc<Mutex<Vec<String>>>, earlier: &str, later: &str) -> bool {
            let events = log.lock().unwrap();
            let position = |event: &str| events.iter().position(|item| item == event);
            matches!(
                (position(earlier), position(later)),
                (Some(first), Some(second)) if first < second
            )
        }

        fn wait_for_typing_call(multiplexer: &FakeMultiplexer, limit: Duration) -> bool {
            Deadline::after(limit).wait_until(|| {
                multiplexer
                    .calls()
                    .iter()
                    .any(|call| call.starts_with("type_when_ready"))
            })
        }
    }

    #[derive(Default)]
    struct RecordingEntryPoints {
        socket_problem: Option<String>,
        entered: Mutex<Vec<String>>,
    }

    impl RecordingEntryPoints {
        fn with_socket_problem() -> Self {
            Self {
                socket_problem: Some("the socket is gone".to_string()),
                ..Self::default()
            }
        }

        fn enter(&self, arguments: &[&str]) {
            let cli = Cli::try_parse_from(["ringleader"].iter().chain(arguments)).unwrap();
            cli.enter(self);
        }

        fn entered(&self) -> Vec<String> {
            self.entered.lock().unwrap().clone()
        }

        fn record(&self, entry: String) -> ExitCode {
            self.entered.lock().unwrap().push(entry);
            ExitCode::SUCCESS
        }
    }

    impl EntryPoints for RecordingEntryPoints {
        fn launch_agent(&self, handoff: &Path) -> ExitCode {
            self.record(format!("launch_agent {}", handoff.display()))
        }

        fn print_version(&self, reply: &VersionReply, mode: OutputMode) -> ExitCode {
            let stdout = CapturedWriter::default();
            let stderr = CapturedWriter::default();
            let mut output = CliOutput::new(Box::new(stdout.clone()), Box::new(stderr), mode);
            let code = output.reply(reply).map_or(CliExit::FAILURE, |()| 0);
            self.record(format!("print_version {code} {}", stdout.text().trim_end()))
        }

        fn socket_problem(&self) -> Option<String> {
            self.socket_problem.clone()
        }

        fn refuse_to_start(&self, problem: &str) -> ExitCode {
            self.record(format!("refuse_to_start {problem}"))
        }

        fn find_session(&self, _cli: &Cli, client: &str) -> ExitCode {
            self.record(format!("find_session {client}"))
        }

        fn show_session_menu(&self, _cli: &Cli, client: &str) -> ExitCode {
            self.record(format!("show_session_menu {client}"))
        }

        fn open_tui(&self, _cli: &Cli) -> ExitCode {
            self.record("open_tui".to_string())
        }

        fn run_command(&self, _cli: &Cli) -> ExitCode {
            self.record("run_command".to_string())
        }
    }

    #[test]
    fn a_lock_held_past_the_patience_exits_three_and_names_the_lock() {
        let harness = CliHarness::new(ServiceFixture::new()).with_lock(FakeLock::held_elsewhere());

        let exit = harness.run(&["delete", "fix-auth"]);

        assert_eq!(exit.code(), CliExit::CONFLICT);
        assert!(
            harness
                .stderr()
                .contains("another Ringleader holds /state/ringleader/lock")
        );
    }

    #[test]
    fn a_mutating_command_waits_for_another_ringleader_to_release_the_lock() {
        let harness =
            CliHarness::new(ServiceFixture::new()).with_lock(FakeLock::held_until_waited());

        let exit = harness.run(&["new", "--name", "Fix Auth", "--repo", "example-project"]);

        assert_eq!(exit, CliExit::Code(0));
        assert_eq!(harness.lock_acquisitions(), ["try", "wait"]);
    }

    #[test]
    fn a_wait_for_the_lock_tells_the_user_why_the_command_pauses() {
        let harness =
            CliHarness::new(ServiceFixture::new()).with_lock(FakeLock::held_until_waited());

        harness.run(&["delete", "fix-auth"]);

        assert!(
            harness
                .stderr()
                .contains("waiting for another Ringleader to finish a change")
        );
    }

    #[test]
    fn a_signal_ends_the_wait_for_the_lock() {
        let harness = CliHarness::new(ServiceFixture::new()).with_lock(FakeLock::held_elsewhere());
        harness.signals().raise(SIGINT);

        let exit = harness.run(&["delete", "fix-auth"]);

        assert!(matches!(exit, CliExit::Interrupted { signal: SIGINT, .. }));
        assert!(!harness.stderr().contains("another Ringleader holds"));
    }

    #[test]
    fn a_read_only_command_takes_no_lock() {
        let harness = CliHarness::new(ServiceFixture::new());

        harness.run(&["list"]);

        assert!(harness.lock_acquisitions().is_empty());
    }

    #[test]
    fn a_mutating_command_takes_the_write_lock() {
        let harness = CliHarness::new(ServiceFixture::new());

        harness.run(&["delete", "fix-auth"]);

        assert_eq!(harness.lock_acquisitions(), ["try"]);
    }

    #[test]
    fn a_free_lock_ignores_the_bound() {
        let harness = CliHarness::new(ServiceFixture::new());

        let exit = harness.run(&[
            "new",
            "--name",
            "Fix Auth",
            "--repo",
            "example-project",
            "--lock-timeout",
            "5",
        ]);

        assert_eq!(exit, CliExit::Code(0));
        assert_eq!(harness.lock_acquisitions(), ["try"]);
        assert!(harness.lock_patiences().is_empty());
        assert!(!harness.stderr().contains("waiting for another Ringleader"));
    }

    #[test]
    fn a_lock_held_past_the_bound_exits_three_and_names_the_lock() {
        let harness = CliHarness::new(ServiceFixture::new()).with_lock(FakeLock::held_elsewhere());

        let exit = harness.run(&["delete", "fix-auth", "--lock-timeout", "10"]);

        assert_eq!(exit.code(), CliExit::CONFLICT);
        assert!(
            harness
                .stderr()
                .contains("another Ringleader holds /state/ringleader/lock")
        );
        assert_eq!(
            harness.lock_patiences(),
            [LockPatience::of(Duration::from_secs(10))]
        );
    }

    #[test]
    fn a_bound_above_the_hold_lets_the_command_run() {
        let harness = CliHarness::new(ServiceFixture::new())
            .with_lock(FakeLock::held_for(LockFlowFixture::LONG_HOLD));

        let exit = harness.run(&[
            "new",
            "--name",
            "Fix Auth",
            "--repo",
            "example-project",
            "--lock-timeout",
            "10",
        ]);

        assert_eq!(exit, CliExit::Code(0));
    }

    #[test]
    fn a_bound_below_the_hold_exits_three() {
        let harness = CliHarness::new(ServiceFixture::new())
            .with_lock(FakeLock::held_for(LockFlowFixture::LONG_HOLD));

        let exit = harness.run(&["delete", "fix-auth", "--lock-timeout", "3"]);

        assert_eq!(exit.code(), CliExit::CONFLICT);
    }

    #[test]
    fn a_lock_released_within_the_bound_lets_the_command_run() {
        let harness =
            CliHarness::new(ServiceFixture::new()).with_lock(FakeLock::held_until_waited());

        let exit = harness.run(&[
            "new",
            "--name",
            "Fix Auth",
            "--repo",
            "example-project",
            "--lock-timeout",
            "10",
        ]);

        assert_eq!(exit, CliExit::Code(0));
        assert_eq!(harness.lock_acquisitions(), ["try", "wait"]);
        assert_eq!(
            harness.lock_patiences(),
            [LockPatience::of(Duration::from_secs(10))]
        );
        assert!(
            harness
                .stderr()
                .contains("waiting for another Ringleader to finish a change")
        );
    }

    #[test]
    fn a_zero_bound_fails_at_once_without_a_wait_note() {
        let harness = CliHarness::new(ServiceFixture::new()).with_lock(FakeLock::held_elsewhere());

        let exit = harness.run(&["delete", "fix-auth", "--lock-timeout", "0"]);

        assert_eq!(exit.code(), CliExit::CONFLICT);
        assert_eq!(harness.lock_acquisitions(), ["try"]);
        assert!(harness.lock_patiences().is_empty());
        assert!(!harness.stderr().contains("waiting for another Ringleader"));
        assert!(
            harness
                .stderr()
                .contains("another Ringleader holds /state/ringleader/lock")
        );
    }

    #[test]
    fn a_change_command_json_reports_a_held_lock_in_the_envelope() {
        let harness = CliHarness::new(ServiceFixture::new()).with_lock(FakeLock::held_elsewhere());

        let exit = harness.run(&["rename", "a", "b", "--json", "--lock-timeout", "0"]);

        let json: serde_json::Value = serde_json::from_str(harness.stdout().trim()).unwrap();
        assert_eq!(json["error"]["code"], "lock-held");
        assert_eq!(harness.stderr(), "");
        assert_eq!(exit.code(), CliExit::CONFLICT);
    }

    #[test]
    fn a_signal_ends_a_bounded_wait_for_the_lock() {
        let harness = CliHarness::new(ServiceFixture::new()).with_lock(FakeLock::held_elsewhere());
        harness.signals().raise(SIGINT);

        let exit = harness.run(&["delete", "fix-auth", "--lock-timeout", "10"]);

        assert!(matches!(exit, CliExit::Interrupted { signal: SIGINT, .. }));
    }

    #[test]
    fn a_command_without_the_write_lock_refuses_the_lock_timeout_flag() {
        assert_eq!(
            ParseFixture::exit_code(&["ringleader", "list", "--lock-timeout", "5"]),
            1
        );
    }

    #[test]
    fn a_negative_lock_timeout_exits_one() {
        assert_eq!(
            ParseFixture::exit_code(&["ringleader", "delete", "a", "--lock-timeout", "-1"]),
            1
        );
    }

    #[test]
    fn a_non_numeric_lock_timeout_exits_one() {
        assert_eq!(
            ParseFixture::exit_code(&["ringleader", "delete", "a", "--lock-timeout", "soon"]),
            1
        );
    }

    #[test]
    fn every_changing_command_parses_the_lock_timeout_flag() {
        let cases = [
            &["new", "--name", "x", "--repo", "r"][..],
            &["delete", "x"][..],
            &["rename", "a", "b"][..],
            &["restart", "--all"][..],
            &["restart", "x"][..],
            &["stop", "x"][..],
            &["stop", "--repo", "r"][..],
            &["repo", "add", "/tmp/repo"][..],
            &["repo", "remove", "x"][..],
        ];
        for arguments in cases {
            let mut words = vec!["ringleader"];
            words.extend_from_slice(arguments);
            words.extend(["--lock-timeout", "5"]);
            Cli::try_parse_from(words.iter().copied())
                .unwrap_or_else(|error| panic!("{arguments:?}: {error}"));
        }
    }

    #[test]
    fn new_types_the_startup_input_before_the_command_exits() {
        let fixture = ServiceFixture::new()
            .with_config(ServiceFixture::config_with_input())
            .with_multiplexer(FakeMultiplexer::new().blocking_typing());
        let typed = fixture.shared_multiplexer();
        let harness = CliHarness::new(fixture);
        LockFlowFixture::release_typing_later(&typed);

        let exit = harness.run(&["new", "--name", "Fix Auth", "--repo", "example-project"]);

        assert_eq!(exit, CliExit::Code(0));
        assert_eq!(typed.typed_when_ready("ringleader-fix-auth"), ["/color"]);
    }

    #[test]
    fn new_notes_that_it_waits_for_the_startup_input() {
        let fixture = ServiceFixture::new()
            .with_config(ServiceFixture::config_with_input())
            .with_multiplexer(FakeMultiplexer::new().blocking_typing());
        let typing = fixture.shared_multiplexer();
        let stderr = CapturedWriter::default();
        let harness = CliHarness::new(fixture).with_stderr(stderr.clone());
        let handle = thread::spawn(move || {
            harness.run(&["new", "--name", "Fix Auth", "--repo", "example-project"])
        });

        let noted = Deadline::after(Duration::from_secs(2)).wait_until(|| {
            stderr
                .text()
                .contains("waiting for the agent to reach its prompt to type its startup input")
        });
        typing.release_typing();
        handle.join().unwrap();

        assert!(noted);
    }

    #[test]
    fn new_without_startup_input_notes_no_wait() {
        let harness = CliHarness::new(ServiceFixture::new());

        let exit = harness.run(&["new", "--name", "Fix Auth", "--repo", "example-project"]);

        assert_eq!(exit, CliExit::Code(0));
        assert!(!harness.stderr().contains("waiting for the agent"));
    }

    #[test]
    fn a_signal_ends_the_startup_input_wait() {
        let fixture = ServiceFixture::new()
            .with_config(ServiceFixture::config_with_input())
            .with_multiplexer(FakeMultiplexer::new().blocking_typing());
        let typing = fixture.shared_multiplexer();
        let harness = CliHarness::new(fixture);
        let signals = harness.signals().clone();
        let started = Instant::now();
        let handle = thread::spawn(move || {
            harness.run(&["new", "--name", "Fix Auth", "--repo", "example-project"])
        });
        assert!(LockFlowFixture::wait_for_typing_call(
            &typing,
            Duration::from_secs(2)
        ));

        signals.raise(SIGINT);
        let exit = handle.join().unwrap();

        assert_eq!(
            exit,
            CliExit::Interrupted {
                signal: SIGINT,
                code: 0
            }
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        typing.release_typing();
    }

    #[test]
    fn a_signal_during_the_command_skips_the_wait_note() {
        let fixture = ServiceFixture::new()
            .with_config(ServiceFixture::config_with_input())
            .with_multiplexer(FakeMultiplexer::new().blocking_typing());
        let typing = fixture.shared_multiplexer();
        let harness = CliHarness::new(fixture);
        let signals = harness.signals().clone();
        typing.add_call_hook("create_session", move || signals.raise(SIGINT));

        harness.run(&["new", "--name", "Fix Auth", "--repo", "example-project"]);

        assert!(!harness.stderr().contains("waiting for the agent"));
        typing.release_typing();
    }

    #[test]
    fn the_write_lock_is_released_before_the_startup_input_is_waited_on() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let create_log = Arc::clone(&log);
        let typing_log = Arc::clone(&log);
        let gate = Arc::new(Gate::closed());
        let multiplexer = FakeMultiplexer::new()
            .with_typing_gate(Arc::clone(&gate))
            .on_call("create_session", move || {
                create_log.lock().unwrap().push("create".to_string());
            })
            .on_call("type_when_ready", move || {
                typing_log.lock().unwrap().push("typing".to_string());
            });
        let lock = FakeLock::free().recording(Arc::clone(&log));
        let fixture = ServiceFixture::new()
            .with_config(ServiceFixture::config_with_input())
            .with_multiplexer(multiplexer);
        let typed = fixture.shared_multiplexer();
        let harness = CliHarness::new(fixture).with_lock(lock);

        let handle = thread::spawn(move || {
            harness.run(&["new", "--name", "Fix Auth", "--repo", "example-project"])
        });

        assert!(LockFlowFixture::wait_for(
            &log,
            "typing",
            Duration::from_secs(2)
        ));
        assert!(
            LockFlowFixture::wait_for(&log, "release", Duration::from_secs(1)),
            "the write lock was still held when the typing wait began"
        );
        assert!(
            LockFlowFixture::recorded_before(&log, "create", "release"),
            "the write lock was released before the use case ran"
        );
        gate.open_now();
        assert_eq!(handle.join().unwrap(), CliExit::Code(0));
        assert_eq!(typed.typed_when_ready("ringleader-fix-auth"), ["/color"]);
    }

    #[test]
    fn the_purge_waits_for_the_startup_input_so_a_signal_cannot_drop_the_task() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let typing_log = Arc::clone(&log);
        let gate = Arc::new(Gate::closed());
        let multiplexer = FakeMultiplexer::new()
            .with_typing_gate(Arc::clone(&gate))
            .on_call("type_when_ready", move || {
                typing_log.lock().unwrap().push("typing".to_string());
            });
        let space = FakeScratchSpace::holding_one_retired().recording_purges(Arc::clone(&log));
        let fixture = ServiceFixture::new()
            .with_config(ServiceFixture::config_with_input())
            .with_multiplexer(multiplexer)
            .with_scratch_space(space);
        let harness = CliHarness::new(fixture);

        let handle = thread::spawn(move || {
            harness.run(&["new", "--name", "Fix Auth", "--repo", "example-project"])
        });

        assert!(LockFlowFixture::wait_for(
            &log,
            "typing",
            Duration::from_secs(2)
        ));
        let purged_early = LockFlowFixture::wait_for(&log, "purge", LockFlowFixture::TYPING_DELAY);
        gate.open_now();
        assert_eq!(handle.join().unwrap(), CliExit::Code(0));
        assert!(!purged_early, "the purge ran before the startup input");
        assert!(LockFlowFixture::recorded_before(&log, "typing", "purge"));
    }

    #[test]
    fn the_write_lock_is_released_before_the_retired_scratch_is_purged() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let lock = FakeLock::free().recording(Arc::clone(&log));
        let space = FakeScratchSpace::holding_one_retired().recording_purges(Arc::clone(&log));
        let harness =
            CliHarness::new(ServiceFixture::new().with_scratch_space(space)).with_lock(lock);

        harness.run(&["new", "--name", "Fix Auth", "--repo", "example-project"]);

        assert!(
            LockFlowFixture::recorded_before(&log, "release", "purge"),
            "the purge ran under the write lock: {:?}",
            log.lock().unwrap()
        );
    }

    struct ParseFixture;

    impl ParseFixture {
        fn exit_code(arguments: &[&str]) -> u8 {
            let error = Cli::try_parse_from(arguments).unwrap_err();
            Cli::exit_code_for(&error)
        }
    }

    #[test]
    fn an_unknown_flag_exits_one() {
        assert_eq!(
            ParseFixture::exit_code(&["ringleader", "--no-such-flag"]),
            1
        );
    }

    #[test]
    fn a_missing_argument_exits_one() {
        assert_eq!(ParseFixture::exit_code(&["ringleader", "delete"]), 1);
    }

    #[test]
    fn delete_does_not_parse_the_json_flag() {
        assert_eq!(
            ParseFixture::exit_code(&["ringleader", "delete", "a", "--json"]),
            1
        );
    }

    #[test]
    fn an_invalid_value_exits_one() {
        assert_eq!(
            ParseFixture::exit_code(&["ringleader", "list", "--json=maybe"]),
            1
        );
    }

    #[test]
    fn restart_with_both_a_name_and_all_exits_one() {
        assert_eq!(
            ParseFixture::exit_code(&["ringleader", "restart", "fix-auth", "--all"]),
            1
        );
    }

    #[test]
    fn stop_with_both_a_name_and_a_repo_exits_one() {
        assert_eq!(
            ParseFixture::exit_code(&[
                "ringleader",
                "stop",
                "fix-auth",
                "--repo",
                "example-project"
            ]),
            1
        );
    }

    #[test]
    fn stop_without_a_name_or_a_repo_exits_one() {
        assert_eq!(ParseFixture::exit_code(&["ringleader", "stop"]), 1);
    }

    #[test]
    fn new_with_a_prompt_and_a_prompt_file_exits_one() {
        assert_eq!(
            ParseFixture::exit_code(&[
                "ringleader",
                "new",
                "--name",
                "fix-auth",
                "--repo",
                "example-project",
                "--prompt",
                "go",
                "--prompt-file",
                "prompt.txt",
            ]),
            1
        );
    }

    #[test]
    fn send_with_a_prompt_and_a_prompt_file_exits_one() {
        assert_eq!(
            ParseFixture::exit_code(&[
                "ringleader",
                "send",
                "fix-auth",
                "go",
                "--prompt-file",
                "prompt.txt",
            ]),
            1
        );
    }

    #[test]
    fn send_without_a_prompt_or_a_prompt_file_exits_one() {
        assert_eq!(
            ParseFixture::exit_code(&["ringleader", "send", "fix-auth"]),
            1
        );
    }

    #[test]
    fn new_in_the_main_checkout_with_a_base_branch_exits_one() {
        assert_eq!(
            ParseFixture::exit_code(&[
                "ringleader",
                "new",
                "--name",
                "fix-auth",
                "--repo",
                "example-project",
                "--main-checkout",
                "--base-branch",
                "develop",
            ]),
            1
        );
    }

    #[test]
    fn restart_without_a_name_or_all_exits_one() {
        assert_eq!(ParseFixture::exit_code(&["ringleader", "restart"]), 1);
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(ParseFixture::exit_code(&["ringleader", "--help"]), 0);
    }

    #[test]
    fn the_finder_arguments_followed_by_a_client_name_enter_the_finder() {
        let entry_points = RecordingEntryPoints::default();
        let words: Vec<String> = Cli::finder_arguments()
            .iter()
            .map(|word| word.to_string_lossy().into_owned())
            .chain(["client-1234".to_string()])
            .collect();
        let arguments: Vec<&str> = words.iter().map(String::as_str).collect();

        entry_points.enter(&arguments);

        assert_eq!(entry_points.entered(), ["find_session client-1234"]);
    }

    #[test]
    fn the_menu_arguments_followed_by_a_client_name_enter_the_session_menu() {
        let entry_points = RecordingEntryPoints::default();
        let words: Vec<String> = Cli::menu_arguments()
            .iter()
            .map(|word| word.to_string_lossy().into_owned())
            .chain(["client-1234".to_string()])
            .collect();
        let arguments: Vec<&str> = words.iter().map(String::as_str).collect();

        entry_points.enter(&arguments);

        assert_eq!(entry_points.entered(), ["show_session_menu client-1234"]);
    }

    #[test]
    fn a_run_that_another_entry_point_takes_runs_no_command() {
        for arguments in [
            ["__find-session", "--client", "client-1234"].as_slice(),
            ["__session-menu", "--client", "client-1234"].as_slice(),
            &[],
        ] {
            let harness = CliHarness::new(ServiceFixture::new());

            let exit = harness.run(arguments);

            assert_eq!(exit, CliExit::Code(0), "{arguments:?}");
            assert_eq!(harness.stdout(), "", "{arguments:?}");
        }
    }

    #[test]
    fn the_hidden_launcher_enters_before_the_socket_check() {
        let entry_points = RecordingEntryPoints::with_socket_problem();

        entry_points.enter(&["__exec-agent", "--handoff", "/run/h.json"]);

        assert_eq!(entry_points.entered(), ["launch_agent /run/h.json"]);
    }

    #[test]
    fn the_version_command_enters_before_the_socket_check() {
        let entry_points = RecordingEntryPoints::with_socket_problem();

        entry_points.enter(&["version"]);

        assert_eq!(
            entry_points.entered(),
            [format!(
                "print_version 0 ringleader {}",
                BuildVersion::CURRENT.text()
            )]
        );
    }

    #[test]
    fn the_version_command_prints_the_line_that_the_version_flag_prints() {
        let entry_points = RecordingEntryPoints::default();

        entry_points.enter(&["version"]);

        let recorded = entry_points.entered().pop().unwrap();
        let line = recorded.strip_prefix("print_version 0 ").unwrap();
        let flag = Cli::try_parse_from(["ringleader", "--version"]).unwrap_err();
        assert_eq!(flag.to_string().trim_end(), line);
    }

    #[test]
    fn version_json_prints_the_pinned_envelope() {
        let entry_points = RecordingEntryPoints::default();

        entry_points.enter(&["version", "--json"]);

        let expected = include_str!("../tests/fixtures/contract/version.json")
            .replace("<build>", BuildVersion::CURRENT.text());
        assert_eq!(
            entry_points.entered(),
            [format!("print_version 0 {}", expected.trim_end())]
        );
    }

    struct DoctorJsonFixture;

    impl DoctorJsonFixture {
        fn healthy() -> ServiceFixture {
            let session = ServiceFixture::session("fix-auth");
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&session))
                .with_workspace(ServiceFixture::workspace_for(&[&session]))
                .with_host(ServiceFixture::host_for(&[&session]))
        }

        fn with_a_problem() -> ServiceFixture {
            ServiceFixture::new().with_config(ServiceFixture::config().with_agents(vec![]))
        }
    }

    #[test]
    fn doctor_json_matches_the_pinned_envelope() {
        let harness = CliHarness::new(DoctorJsonFixture::healthy());

        let exit = harness.run(&["doctor", "--json"]);

        let expected = include_str!("../tests/fixtures/contract/doctor.json")
            .replace("<build>", BuildVersion::CURRENT.text());
        assert_eq!(harness.stdout(), expected);
        assert_eq!(exit, CliExit::Code(0));
    }

    #[test]
    fn a_doctor_that_succeeds_despite_a_signal_exits_with_its_own_code() {
        let harness = CliHarness::new(DoctorJsonFixture::healthy());
        harness.signals().raise(SIGINT);

        let exit = harness.run(&["doctor"]);

        assert_eq!(exit, CliExit::Code(0));
        assert!(!harness.stderr().contains("interrupted"));
    }

    #[test]
    fn doctor_json_exits_one_for_a_problem() {
        let harness = CliHarness::new(DoctorJsonFixture::with_a_problem());

        let exit = harness.run(&["doctor", "--json"]);

        assert_eq!(exit, CliExit::Code(1));
    }

    #[test]
    fn doctor_json_reports_a_problem_finding() {
        let harness = CliHarness::new(DoctorJsonFixture::with_a_problem());

        harness.run(&["doctor", "--json"]);

        let json: serde_json::Value = serde_json::from_str(harness.stdout().trim()).unwrap();
        let findings = json["result"]["findings"].as_array().unwrap();
        assert!(
            findings
                .iter()
                .any(|finding| finding["severity"] == "problem")
        );
    }

    #[test]
    fn a_socket_problem_refuses_every_entry_but_doctor_and_version() {
        let entry_points = RecordingEntryPoints::with_socket_problem();

        entry_points.enter(&[]);
        entry_points.enter(&["__find-session", "--client", "client-1234"]);
        entry_points.enter(&["__session-menu", "--client", "client-1234"]);
        entry_points.enter(&["doctor"]);
        entry_points.enter(&["version"]);

        let expected = [
            "refuse_to_start the socket is gone",
            "refuse_to_start the socket is gone",
            "refuse_to_start the socket is gone",
            "run_command",
        ]
        .into_iter()
        .map(str::to_string)
        .chain([format!(
            "print_version 0 ringleader {}",
            BuildVersion::CURRENT.text()
        )])
        .collect::<Vec<_>>();
        assert_eq!(entry_points.entered(), expected);
    }

    #[test]
    fn no_subcommand_enters_the_tui_and_a_subcommand_the_cli() {
        let entry_points = RecordingEntryPoints::default();

        entry_points.enter(&[]);
        entry_points.enter(&["list"]);

        assert_eq!(entry_points.entered(), ["open_tui", "run_command"]);
    }

    #[test]
    fn global_options_are_accepted_before_or_after_a_subcommand() {
        let before =
            Cli::try_parse_from(["ringleader", "--config", "/tmp/c.toml", "list"]).unwrap();
        let after = Cli::try_parse_from(["ringleader", "list", "--config", "/tmp/c.toml"]).unwrap();

        assert_eq!(before.config_path(), Some(Path::new("/tmp/c.toml")));
        assert_eq!(after.config_path(), Some(Path::new("/tmp/c.toml")));
    }

    #[test]
    fn only_changing_commands_take_the_write_lock() {
        let list = Cli::try_parse_from(["ringleader", "list"]).unwrap();
        let delete = Cli::try_parse_from(["ringleader", "delete", "x"]).unwrap();

        assert!(!list.is_mutating());
        assert!(delete.is_mutating());
    }

    #[test]
    fn restart_all_takes_the_write_lock() {
        let restart_all = Cli::try_parse_from(["ringleader", "restart", "--all"]).unwrap();

        assert!(restart_all.is_mutating());
    }

    #[test]
    fn stop_of_a_repo_takes_the_write_lock() {
        let stop =
            Cli::try_parse_from(["ringleader", "stop", "--repo", "example-project"]).unwrap();

        assert!(stop.is_mutating());
    }

    #[test]
    fn stop_takes_the_write_lock() {
        let stop = Cli::try_parse_from(["ringleader", "stop", "fix-auth"]).unwrap();

        assert!(stop.is_mutating());
    }

    #[test]
    fn stop_and_delete_need_at_least_one_name() {
        for command in ["stop", "delete"] {
            assert_eq!(
                ParseFixture::exit_code(&["ringleader", command]),
                CliExit::FAILURE
            );
        }
    }
}
