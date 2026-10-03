use thiserror::Error;

pub trait ReturnBinding {
    fn install(&self) -> Result<(), BindingError>;
    fn restore(&self) -> Result<(), BindingError>;
    fn repair_stale(&self) -> Result<(), BindingError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BindingError {
    #[error("tmux {subcommand} failed: {detail}")]
    Failed { subcommand: String, detail: String },
    #[error("tmux {subcommand} timed out after {seconds} seconds")]
    TimedOut { subcommand: String, seconds: u64 },
    #[error("Ctrl-Q return binding not installed: {reason}")]
    Unavailable { reason: String },
    #[error("could not restore the previous Ctrl-Q binding, rebind it by hand: {saved_line}")]
    NotRestored { saved_line: String },
    #[error("kept the saved Ctrl-Q binding for a later retry: {detail}")]
    RecordKept { detail: String, saved_line: String },
    #[error("cannot write {detail}")]
    Record { detail: String },
}

impl BindingError {
    pub fn is_timeout(&self) -> bool {
        matches!(self, BindingError::TimedOut { .. })
    }
}
