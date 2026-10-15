use std::path::PathBuf;
use std::sync::Mutex;

use crate::application::ports::{RegistryError, SessionRegistry};
use crate::domain::Session;

pub struct FakeSessionRegistry {
    state: Mutex<RegistryState>,
}

#[derive(Default)]
struct RegistryState {
    sessions: Vec<Session>,
    saves: usize,
    failing_saves: bool,
    load_error: Option<RegistryError>,
    moved_aside: bool,
}

impl FakeSessionRegistry {
    pub fn empty() -> Self {
        Self::with_sessions(Vec::new())
    }

    pub fn with_sessions(sessions: Vec<Session>) -> Self {
        Self {
            state: Mutex::new(RegistryState {
                sessions,
                ..RegistryState::default()
            }),
        }
    }

    pub fn failing_saves(self) -> Self {
        self.state.lock().unwrap().failing_saves = true;
        self
    }

    pub fn failing_load(self, error: RegistryError) -> Self {
        self.state.lock().unwrap().load_error = Some(error);
        self
    }

    pub fn break_loads(&self, error: RegistryError) {
        self.state.lock().unwrap().load_error = Some(error);
    }

    pub fn sessions(&self) -> Vec<Session> {
        self.state.lock().unwrap().sessions.clone()
    }

    pub fn save_count(&self) -> usize {
        self.state.lock().unwrap().saves
    }

    pub fn was_moved_aside(&self) -> bool {
        self.state.lock().unwrap().moved_aside
    }
}

impl SessionRegistry for FakeSessionRegistry {
    fn load(&self) -> Result<Vec<Session>, RegistryError> {
        let state = self.state.lock().unwrap();
        match &state.load_error {
            Some(error) => Err(error.clone()),
            None => Ok(state.sessions.clone()),
        }
    }

    fn save(&self, sessions: &[Session]) -> Result<(), RegistryError> {
        let mut state = self.state.lock().unwrap();
        if state.failing_saves {
            return Err(RegistryError::Io {
                path: PathBuf::from("/state/ringleader/sessions.json"),
                detail: "disk full".to_string(),
            });
        }
        state.sessions = sessions.to_vec();
        state.saves += 1;
        Ok(())
    }

    fn move_aside_corrupt(&self) -> Result<PathBuf, RegistryError> {
        let mut state = self.state.lock().unwrap();
        state.moved_aside = true;
        state.load_error = None;
        state.sessions.clear();
        Ok(PathBuf::from("/state/ringleader/sessions.json.corrupt-1"))
    }
}
