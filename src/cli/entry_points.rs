use std::path::Path;
use std::process::ExitCode;

use crate::cli::Cli;

pub trait EntryPoints {
    fn launch_agent(&self, handoff: &Path) -> ExitCode;
    fn socket_problem(&self) -> Option<String>;
    fn refuse_to_start(&self, problem: &str) -> ExitCode;
    fn find_session(&self, cli: &Cli, client: &str) -> ExitCode;
    fn open_tui(&self, cli: &Cli) -> ExitCode;
    fn run_command(&self, cli: &Cli) -> ExitCode;
}
