mod cli_output;
mod delete_report;
mod list_report;
mod repo_argument;
mod session_commands;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

pub use cli_output::CliOutput;
use session_commands::SessionCommands;

use crate::adapters::signal_flags::SignalFlags;
use crate::application::ports::{Lock, LockError};
use crate::application::{CorruptRegistry, Pending, ServiceError, SessionService};

#[derive(Debug, Parser)]
#[command(
    name = "ringleader",
    version,
    about = "A tmux session manager for Claude and opencode"
)]
pub struct Cli {
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,
    #[arg(long, global = true, value_name = "LEVEL")]
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
    #[command(name = "__exec-agent", hide = true)]
    ExecAgent {
        #[arg(long, value_name = "PATH")]
        handoff: PathBuf,
    },
}

#[derive(Debug, Args)]
struct ListArgs {
    #[arg(long)]
    json: bool,
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
    #[arg(long)]
    keep_branch: bool,
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

    pub fn is_mutating(&self) -> bool {
        matches!(
            self.command,
            Some(
                CliCommand::New(_)
                    | CliCommand::Delete(_)
                    | CliCommand::Rename(_)
                    | CliCommand::Restart(_)
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
            session.output.error(&error.to_string());
            match error {
                LockError::Held { .. } => CliExit::Code(ServiceError::CONFLICT_EXIT),
                LockError::Io { .. } => CliExit::Code(Self::FAILURE_EXIT),
            }
        })
    }

    pub fn execute(&self, service: &mut SessionService, session: &mut CliSession) -> CliExit {
        if let Some(error) = service.read_only_reason() {
            session.output.error(&error.to_string());
            return CliExit::Code(Self::FAILURE_EXIT);
        }
        let recovery = if self.is_mutating() {
            CorruptRegistry::MoveAside
        } else {
            CorruptRegistry::Report
        };
        match service.load_registry(recovery) {
            Ok(Some(warning)) => session.output.error(&warning),
            Ok(None) => {}
            Err(error) => {
                session.output.error(&error.to_string());
                return CliExit::Code(Self::FAILURE_EXIT);
            }
        }
        let result = self.dispatch(service, session);
        session.finish(result)
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
            Some(CliCommand::ExecAgent { .. }) | None => Ok(0),
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

pub struct CliSession {
    output: CliOutput,
    signals: SignalFlags,
}

impl CliSession {
    const SIGNAL_EXIT_BASE: u8 = 128;

    pub fn new(output: CliOutput, signals: SignalFlags) -> Self {
        Self { output, signals }
    }

    pub fn output(&mut self) -> &mut CliOutput {
        &mut self.output
    }

    fn stop_if_signaled(&self) -> Result<(), CommandStop> {
        match self.signals.raised() {
            Some(signal) => Err(CommandStop::Interrupted(signal)),
            None => Ok(()),
        }
    }

    fn wait<T: Send + 'static>(&self, pending: &Pending<T>) -> Result<T, CommandStop> {
        match pending.wait_unless(|| self.signals.raised().is_some()) {
            Some(result) => result.map_err(CommandStop::Failed),
            None => Err(CommandStop::Interrupted(
                self.signals.raised().unwrap_or_default(),
            )),
        }
    }

    fn finish(&mut self, result: Result<u8, CommandStop>) -> CliExit {
        let code = match result {
            Ok(code) => code,
            Err(CommandStop::Failed(error)) => {
                self.output.error(&error.to_string());
                error.exit_code()
            }
            Err(CommandStop::Interrupted(signal)) => {
                return CliExit::Interrupted {
                    signal,
                    code: Self::SIGNAL_EXIT_BASE.saturating_add(u8::try_from(signal).unwrap_or(0)),
                };
            }
        };
        match self.signals.raised() {
            Some(signal) => {
                self.output
                    .error("interrupted, the command finished before stopping");
                CliExit::Interrupted { signal, code }
            }
            None => CliExit::Code(code),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliExit {
    Code(u8),
    Interrupted { signal: i32, code: u8 },
}

impl CliExit {
    pub fn code(&self) -> u8 {
        match self {
            CliExit::Code(code) | CliExit::Interrupted { code, .. } => *code,
        }
    }
}

enum CommandStop {
    Failed(ServiceError),
    Interrupted(i32),
}

impl From<ServiceError> for CommandStop {
    fn from(error: ServiceError) -> Self {
        CommandStop::Failed(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
