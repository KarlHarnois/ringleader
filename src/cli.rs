mod cli_output;
mod cli_session;
mod delete_report;
mod list_report;
mod repo_argument;
mod repo_commands;
mod session_commands;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

pub use cli_output::CliOutput;
use cli_session::CommandStop;
pub use cli_session::{CliExit, CliSession};
use repo_commands::RepoCommands;
use session_commands::SessionCommands;

use crate::application::ports::{Lock, LockError};
use crate::application::{CorruptRegistry, ServiceError, SessionService};

#[derive(Debug, Parser)]
#[command(
    name = "ringleader",
    version,
    about = "A tmux session manager for Claude and opencode"
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
    #[command(about = "Create a session in a new worktree")]
    New(NewArgs),
    #[command(about = "Delete a session, its worktree, and its branch")]
    Delete(DeleteArgs),
    #[command(about = "Attach to a session")]
    Attach(NameArgs),
    #[command(about = "Rename a session and its branch")]
    Rename(RenameArgs),
    #[command(about = "Restart an exited or stopped session")]
    Restart(NameArgs),
    #[command(about = "Add, remove, or list repos")]
    Repo {
        #[command(subcommand)]
        action: RepoAction,
    },
    #[command(about = "Check tmux, git, the config, and every session")]
    Doctor(JsonArgs),
    #[command(name = "__exec-agent", hide = true)]
    ExecAgent {
        #[arg(long, value_name = "PATH")]
        handoff: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum RepoAction {
    #[command(about = "Pin a repo as a [[repos]] entry")]
    Add(RepoAddArgs),
    #[command(about = "Remove a [[repos]] entry")]
    Remove(NameArgs),
    #[command(about = "List the repos the picker shows")]
    List,
}

#[derive(Debug, Args)]
struct RepoAddArgs {
    path: String,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    base_branch: Option<String>,
}

#[derive(Debug, Args)]
struct JsonArgs {
    #[arg(long)]
    json: bool,
}

type ListArgs = JsonArgs;

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
    #[arg(long)]
    prompt: Option<String>,
}

#[derive(Debug, Args)]
struct DeleteArgs {
    name: String,
    #[arg(long)]
    force: bool,
    #[arg(long)]
    forget: bool,
}

#[derive(Debug, Args)]
struct NameArgs {
    name: String,
}

#[derive(Debug, Args)]
struct RenameArgs {
    old: String,
    new: String,
}

impl Cli {
    const INVALID_ARGUMENTS_EXIT: u8 = 1;
    const FAILURE_EXIT: u8 = 1;

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

    pub fn config_path(&self) -> Option<&Path> {
        self.config.as_deref()
    }

    pub fn log_level(&self) -> Option<&str> {
        self.log_level.as_deref()
    }

    pub fn exec_agent_handoff(&self) -> Option<&Path> {
        match &self.command {
            Some(CliCommand::ExecAgent { handoff }) => Some(handoff),
            _ => None,
        }
    }

    pub fn runs_tui(&self) -> bool {
        self.command.is_none()
    }

    pub fn is_doctor(&self) -> bool {
        matches!(self.command, Some(CliCommand::Doctor(_)))
    }

    pub fn is_mutating(&self) -> bool {
        matches!(
            self.command,
            Some(
                CliCommand::New(_)
                    | CliCommand::Delete(_)
                    | CliCommand::Rename(_)
                    | CliCommand::Restart(_)
                    | CliCommand::Repo {
                        action: RepoAction::Add(_) | RepoAction::Remove(_)
                    }
            )
        )
    }

    pub fn take_write_lock(
        &self,
        lock: &mut dyn Lock,
        session: &mut CliSession,
    ) -> Result<(), CliExit> {
        if !self.is_mutating() {
            return Ok(());
        }
        lock.acquire_for_write().map_err(|error| {
            session.output().error(&error.to_string());
            match error {
                LockError::Held { .. } => CliExit::Code(ServiceError::CONFLICT_EXIT),
                LockError::Io { .. } => CliExit::Code(Self::FAILURE_EXIT),
            }
        })
    }

    pub fn execute(
        &self,
        service: &mut SessionService,
        session: &mut CliSession,
        lock: &mut dyn Lock,
    ) -> CliExit {
        if let Some(CliCommand::Doctor(args)) = &self.command {
            return CliExit::Code(Self::doctor(service, session, args));
        }
        if let Some(error) = service.read_only_reason() {
            session.output().error(&error.to_string());
            return CliExit::Code(Self::FAILURE_EXIT);
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
                session.output().error(&error.to_string());
                return CliExit::Code(Self::FAILURE_EXIT);
            }
        }
        let result = self.dispatch(service, session);
        session.wait_for_startup_input(service, lock);
        session.finish(result)
    }

    fn restore_server_socket(service: &SessionService, session: &mut CliSession) {
        match service.restore_server_socket() {
            Ok(Some(note)) => session.output().note(&note),
            Ok(None) => {}
            Err(error) => session.output().error(&error.to_string()),
        }
    }

    fn doctor(service: &SessionService, session: &mut CliSession, args: &JsonArgs) -> u8 {
        let report = service.doctor();
        if args.json {
            session.output().line(&report.json());
        } else {
            for line in report.lines() {
                session.output().line(&line);
            }
        }
        u8::from(report.has_problems())
    }

    fn dispatch(
        &self,
        service: &mut SessionService,
        session: &mut CliSession,
    ) -> Result<u8, CommandStop> {
        let mut commands = SessionCommands::new(service, session);
        match &self.command {
            Some(CliCommand::List(args)) => commands.list(args),
            Some(CliCommand::New(args)) => commands.create(args),
            Some(CliCommand::Delete(args)) => commands.delete(args),
            Some(CliCommand::Attach(args)) => commands.attach(&args.name),
            Some(CliCommand::Rename(args)) => commands.rename(args),
            Some(CliCommand::Restart(args)) => commands.restart(&args.name),
            Some(CliCommand::Repo { action }) => {
                let mut repos = RepoCommands::new(service, session);
                match action {
                    RepoAction::Add(args) => repos.add(args),
                    RepoAction::Remove(args) => repos.remove(&args.name),
                    RepoAction::List => repos.list(),
                }
            }
            Some(CliCommand::Doctor(_) | CliCommand::ExecAgent { .. }) | None => Ok(0),
        }
    }

    fn exit_code_for(error: &clap::Error) -> u8 {
        if error.use_stderr() {
            Self::INVALID_ARGUMENTS_EXIT
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
        CapturedWriter, CliHarness, Deadline, FakeLock, FakeMultiplexer, Gate, ServiceFixture,
    };

    struct LockFlowFixture;

    impl LockFlowFixture {
        const TYPING_DELAY: Duration = Duration::from_millis(200);

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

    #[test]
    fn a_mutating_command_against_a_held_lock_exits_three_and_names_the_lock() {
        let harness = CliHarness::new(ServiceFixture::new()).with_lock(FakeLock::held_elsewhere());

        let exit = harness.run(&["delete", "fix-auth"]);

        assert_eq!(exit.code(), ServiceError::CONFLICT_EXIT);
        assert!(
            harness
                .stderr()
                .contains("another instance is running, it holds /state/ringleader/lock")
        );
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

        assert_eq!(harness.lock_acquisitions(), ["write"]);
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
    fn an_invalid_value_exits_one() {
        assert_eq!(
            ParseFixture::exit_code(&["ringleader", "list", "--json=maybe"]),
            1
        );
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(ParseFixture::exit_code(&["ringleader", "--help"]), 0);
    }

    #[test]
    fn the_hidden_launcher_subcommand_takes_a_handoff_path() {
        let cli = Cli::try_parse_from(["ringleader", "__exec-agent", "--handoff", "/run/h.json"])
            .unwrap();

        assert_eq!(cli.exec_agent_handoff(), Some(Path::new("/run/h.json")));
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
}
