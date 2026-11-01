use thiserror::Error;

use crate::domain::ProcessSample;

pub trait ProcessMeter: Send + Sync {
    fn sample(&self) -> Result<ProcessSample, ProcessMeterError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProcessMeterError {
    #[error("could not read the process table: {detail}")]
    Unreadable { detail: String },
}
