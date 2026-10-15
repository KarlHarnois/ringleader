use std::path::Path;

use thiserror::Error;

pub trait TerminalEmulator: Send + Sync {
    fn open_window(&self, directory: &Path) -> Result<(), TerminalEmulatorError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TerminalEmulatorError {
    #[error("no graphical display is available, {reason}")]
    NoDisplay { reason: String },
    #[error("no terminal to open, {reason}")]
    NoTerminal { reason: String },
    #[error("cannot start {program}: {reason}")]
    StartFailed { program: String, reason: String },
}
