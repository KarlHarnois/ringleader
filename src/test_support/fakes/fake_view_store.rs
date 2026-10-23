use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::application::ports::{ViewState, ViewStore, ViewStoreError};
use crate::domain::{RepoGroup, SessionFilter};

#[derive(Clone, Default)]
pub struct FakeViewStore {
    state: Arc<Mutex<StoredView>>,
}

#[derive(Default)]
struct StoredView {
    state: ViewState,
    saves: usize,
    load_failure: Option<String>,
    save_failure: Option<String>,
}

impl FakeViewStore {
    const PATH: &'static str = "/state/ringleader/view.json";

    pub fn holding(groups: &[RepoGroup]) -> Self {
        Self::holding_filter(groups, SessionFilter::All)
    }

    pub fn holding_filter(groups: &[RepoGroup], filter: SessionFilter) -> Self {
        let store = Self::default();
        store.state.lock().unwrap().state =
            ViewState::new(groups.iter().cloned().collect::<HashSet<_>>(), filter);
        store
    }

    pub fn failing_loads(detail: &str) -> Self {
        Self::failing(detail, |view, detail| view.load_failure = Some(detail))
    }

    pub fn failing_saves(detail: &str) -> Self {
        Self::failing(detail, |view, detail| view.save_failure = Some(detail))
    }

    fn failing(detail: &str, set: impl FnOnce(&mut StoredView, String)) -> Self {
        let store = Self::default();
        set(&mut store.state.lock().unwrap(), detail.to_string());
        store
    }

    pub fn folded(&self) -> HashSet<RepoGroup> {
        self.state.lock().unwrap().state.folded().clone()
    }

    pub fn filter(&self) -> SessionFilter {
        self.state.lock().unwrap().state.filter()
    }

    pub fn save_count(&self) -> usize {
        self.state.lock().unwrap().saves
    }

    fn fail_with(failure: &Option<String>) -> Result<(), ViewStoreError> {
        match failure {
            Some(detail) => Err(ViewStoreError::Io {
                path: PathBuf::from(Self::PATH),
                detail: detail.clone(),
            }),
            None => Ok(()),
        }
    }
}

impl ViewStore for FakeViewStore {
    fn load(&self) -> Result<ViewState, ViewStoreError> {
        let state = self.state.lock().unwrap();
        Self::fail_with(&state.load_failure)?;
        Ok(state.state.clone())
    }

    fn save(&self, state: &ViewState) -> Result<(), ViewStoreError> {
        let mut stored = self.state.lock().unwrap();
        Self::fail_with(&stored.save_failure)?;
        stored.state = state.clone();
        stored.saves += 1;
        Ok(())
    }
}
