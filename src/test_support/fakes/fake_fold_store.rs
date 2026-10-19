use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::application::ports::{FoldStore, FoldStoreError};
use crate::domain::RepoGroup;

#[derive(Clone, Default)]
pub struct FakeFoldStore {
    state: Arc<Mutex<FoldState>>,
}

#[derive(Default)]
struct FoldState {
    folded: HashSet<RepoGroup>,
    saves: usize,
    load_failure: Option<String>,
    save_failure: Option<String>,
}

impl FakeFoldStore {
    const PATH: &'static str = "/state/ringleader/folds.json";

    pub fn holding(groups: &[RepoGroup]) -> Self {
        let store = Self::default();
        store.state.lock().unwrap().folded = groups.iter().cloned().collect();
        store
    }

    pub fn failing_loads(detail: &str) -> Self {
        let store = Self::default();
        store.state.lock().unwrap().load_failure = Some(detail.to_string());
        store
    }

    pub fn failing_saves(detail: &str) -> Self {
        let store = Self::default();
        store.state.lock().unwrap().save_failure = Some(detail.to_string());
        store
    }

    pub fn folded(&self) -> HashSet<RepoGroup> {
        self.state.lock().unwrap().folded.clone()
    }

    pub fn save_count(&self) -> usize {
        self.state.lock().unwrap().saves
    }

    fn fail_with(failure: &Option<String>) -> Result<(), FoldStoreError> {
        match failure {
            Some(detail) => Err(FoldStoreError::Io {
                path: PathBuf::from(Self::PATH),
                detail: detail.clone(),
            }),
            None => Ok(()),
        }
    }
}

impl FoldStore for FakeFoldStore {
    fn load(&self) -> Result<HashSet<RepoGroup>, FoldStoreError> {
        let state = self.state.lock().unwrap();
        Self::fail_with(&state.load_failure)?;
        Ok(state.folded.clone())
    }

    fn save(&self, folded: &HashSet<RepoGroup>) -> Result<(), FoldStoreError> {
        let mut state = self.state.lock().unwrap();
        Self::fail_with(&state.save_failure)?;
        state.folded = folded.clone();
        state.saves += 1;
        Ok(())
    }
}
