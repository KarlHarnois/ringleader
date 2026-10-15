use std::ffi::{OsStr, OsString};
use std::path::Path;

use super::git_invocation::GitInvocation;
use super::git_porcelain::GitPorcelain;
use crate::adapters::BoundedCommand;
use crate::adapters::child_process::bounded_command::CommandError;
use crate::adapters::child_process::command_output::CommandOutput;
use crate::application::ports::WorkspaceError;

pub struct GitRunner {
    program: OsString,
    overrides: Vec<(OsString, OsString)>,
    bounded: BoundedCommand,
}

impl GitRunner {
    const NOTHING_FOUND_EXIT: i32 = 1;

    pub fn new(program: &OsStr, bounded: BoundedCommand) -> Self {
        Self {
            program: program.to_os_string(),
            overrides: Vec::new(),
            bounded,
        }
    }

    #[cfg(test)]
    pub fn with_environment(self, overrides: Vec<(OsString, OsString)>) -> Self {
        Self { overrides, ..self }
    }

    pub fn run(&self, invocation: &GitInvocation) -> Result<CommandOutput, WorkspaceError> {
        let command = invocation.command(&self.program, &self.overrides);
        self.bounded
            .run(command, &invocation.call())
            .map_err(|error| match error {
                CommandError::TimedOut { seconds, .. } => WorkspaceError::TimedOut {
                    subcommand: invocation.subcommand().to_string(),
                    seconds,
                },
                CommandError::Spawn { detail, .. } => WorkspaceError::Failed {
                    subcommand: invocation.subcommand().to_string(),
                    status: "not started".to_string(),
                    stderr: detail,
                },
            })
    }

    pub fn run_checked(&self, invocation: &GitInvocation) -> Result<GitPorcelain, WorkspaceError> {
        let output = self.run(invocation)?;
        if !output.succeeded() {
            return Err(Self::failure(invocation, &output));
        }
        Ok(GitPorcelain::new(output.stdout()))
    }

    pub fn lookup(
        &self,
        invocation: &GitInvocation,
    ) -> Result<Option<GitPorcelain>, WorkspaceError> {
        let output = self.run(invocation)?;
        if output.succeeded() {
            return Ok(Some(GitPorcelain::new(output.stdout())));
        }
        if output.exit_code() == Some(Self::NOTHING_FOUND_EXIT) {
            return Ok(None);
        }
        Err(Self::failure(invocation, &output))
    }

    pub fn resolve(
        &self,
        location: &Path,
        revision: &str,
    ) -> Result<Option<String>, WorkspaceError> {
        let invocation =
            GitInvocation::in_repo(location, "rev-parse").args(["--verify", "-q", revision]);
        self.resolved_from(&invocation)
    }

    pub fn resolved_from(
        &self,
        invocation: &GitInvocation,
    ) -> Result<Option<String>, WorkspaceError> {
        let found = self.lookup(invocation)?;
        Ok(found.map(|porcelain| porcelain.first_line().to_string()))
    }

    fn failure(invocation: &GitInvocation, output: &CommandOutput) -> WorkspaceError {
        WorkspaceError::Failed {
            subcommand: invocation.subcommand().to_string(),
            status: output.status_label(),
            stderr: output.stderr_text(),
        }
    }
}
