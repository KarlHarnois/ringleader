use std::path::{Path, PathBuf};
use std::time::SystemTime;

use thiserror::Error;

pub trait BuildCache: Send + Sync {
    fn last_built(&self, directory: &Path) -> Option<SystemTime>;
    fn copy(&self, source: &Path, destination: &Path) -> Result<(), BuildCacheError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BuildCacheError {
    #[error("cannot copy the build cache {origin} to {destination}: {detail}")]
    Copy {
        origin: PathBuf,
        destination: PathBuf,
        detail: String,
    },
    #[error("kept the build cache {destination}, which already exists")]
    Occupied { destination: PathBuf },
}
