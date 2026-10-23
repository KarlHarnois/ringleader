use std::collections::HashSet;
use std::path::PathBuf;

use thiserror::Error;

use crate::domain::{RepoGroup, SessionFilter};

pub trait ViewStore {
    fn load(&self) -> Result<ViewState, ViewStoreError>;
    fn save(&self, state: &ViewState) -> Result<(), ViewStoreError>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewState {
    folded: HashSet<RepoGroup>,
    filter: SessionFilter,
}

impl ViewState {
    pub fn new(folded: HashSet<RepoGroup>, filter: SessionFilter) -> Self {
        Self { folded, filter }
    }

    pub fn folded(&self) -> &HashSet<RepoGroup> {
        &self.folded
    }

    pub fn filter(&self) -> SessionFilter {
        self.filter
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ViewStoreError {
    #[error("{path} is corrupt: {detail}")]
    Corrupt { path: PathBuf, detail: String },
    #[error("cannot access {path}: {detail}")]
    Io { path: PathBuf, detail: String },
}
