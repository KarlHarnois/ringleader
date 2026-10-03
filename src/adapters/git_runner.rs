use std::ffi::{OsStr, OsString};
use std::path::Path;

use super::bounded_command::{BoundedCommand, CommandError};
use super::command_output::CommandOutput;
use super::git_invocation::GitInvocation;
use super::git_porcelain::GitPorcelain;
use crate::application::ports::WorkspaceError;

pub struct GitRunner {
    program: OsString,
    overrides: Vec<(OsString, OsString)>,
    bounded: BoundedCommand,
}

impl GitRunner {
    const MISSING_REF_EXIT: i32 = 1;

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
            return Err(WorkspaceError::Failed {
                subcommand: invocation.subcommand().to_string(),
                status: output.status_label(),
                stderr: output.stderr_text(),
            });
        }
        Ok(GitPorcelain::new(output.stdout()))
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
        let output = self.run(invocation)?;
        if output.succeeded() {
            return Ok(Some(
                GitPorcelain::new(output.stdout()).first_line().to_string(),
            ));
        }
        if output.exit_code() == Some(Self::MISSING_REF_EXIT) {
            return Ok(None);
        }
        Err(WorkspaceError::Failed {
            subcommand: invocation.subcommand().to_string(),
            status: output.status_label(),
            stderr: output.stderr_text(),
        })
    }
}
