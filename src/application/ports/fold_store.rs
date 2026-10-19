use std::collections::HashSet;
use std::path::PathBuf;

use thiserror::Error;

use crate::domain::RepoGroup;

pub trait FoldStore {
    fn load(&self) -> Result<HashSet<RepoGroup>, FoldStoreError>;
    fn save(&self, folded: &HashSet<RepoGroup>) -> Result<(), FoldStoreError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FoldStoreError {
    #[error("{path} is corrupt: {detail}")]
    Corrupt { path: PathBuf, detail: String },
    #[error("cannot access {path}: {detail}")]
    Io { path: PathBuf, detail: String },
}
