use std::path::PathBuf;

use thiserror::Error;

use crate::domain::ThemeOverrides;

pub trait ThemeSource: Send + Sync {
    fn load(&self) -> Result<ThemeOverrides, ThemeError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ThemeError {
    #[error("{path}: {detail}")]
    Parse { path: PathBuf, detail: String },
    #[error("{path}: {}", failures.join("; "))]
    Invalid {
        path: PathBuf,
        failures: Vec<String>,
    },
    #[error("cannot read {path}: {detail}")]
    Io { path: PathBuf, detail: String },
}
