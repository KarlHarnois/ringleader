use thiserror::Error;

use super::tmux_output::TmuxOutput;
use crate::adapters::child_process::bounded_command::CommandError;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TmuxError {
    #[error("{subcommand} failed: {detail}")]
    Failed { subcommand: String, detail: String },
    #[error("{subcommand} timed out after {seconds} seconds")]
    TimedOut { subcommand: String, seconds: u64 },
}

impl TmuxError {
    pub fn failed(subcommand: &str, output: &TmuxOutput) -> Self {
        let detail = match output.stderr() {
            "" => "tmux printed no confirmation".to_string(),
            stderr => stderr.to_string(),
        };
        TmuxError::Failed {
            subcommand: subcommand.to_string(),
            detail,
        }
    }
}

impl From<CommandError> for TmuxError {
    fn from(error: CommandError) -> Self {
        match error {
            CommandError::TimedOut {
                subcommand,
                seconds,
            } => TmuxError::TimedOut {
                subcommand,
                seconds,
            },
            CommandError::Spawn { subcommand, detail } => TmuxError::Failed { subcommand, detail },
        }
    }
}
