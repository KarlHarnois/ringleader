use std::path::PathBuf;

use thiserror::Error;

use crate::domain::Session;

pub trait SessionRegistry {
    fn load(&self) -> Result<Vec<Session>, RegistryError>;
    fn save(&self, sessions: &[Session]) -> Result<(), RegistryError>;
    fn move_aside_corrupt(&self) -> Result<PathBuf, RegistryError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RegistryError {
    #[error("{path} is corrupt: {detail}")]
    Corrupt { path: PathBuf, detail: String },
    #[error(
        "sessions.json has schema version {found}, but this Ringleader reads up to {supported}. Upgrade Ringleader."
    )]
    NewerSchema { found: u64, supported: u64 },
    #[error("cannot access {path}: {detail}")]
    Io { path: PathBuf, detail: String },
}
