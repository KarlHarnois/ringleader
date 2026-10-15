use std::path::Path;

use thiserror::Error;

pub trait TerminalEmulator: Send + Sync {
    fn open_window(&self, directory: &Path) -> Result<(), TerminalEmulatorError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TerminalEmulatorError {
    #[error("no graphical display is available, WAYLAND_DISPLAY and DISPLAY are unset or empty")]
    NoDisplay,
    #[error(
        "no terminal to open, xdg-terminal-exec is not on PATH and $TERMINAL is unset or empty"
    )]
    NoTerminal,
    #[error("cannot start {program}: {reason}")]
    StartFailed { program: String, reason: String },
}
