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
    const MOVED_ASIDE_PATH: &str = "/state/ringleader/sessions.json.corrupt-1";

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

    pub fn with_moved_aside_copy(self) -> Self {
        self.state.lock().unwrap().moved_aside = true;
        self
    }

    pub fn failing_load(self, error: RegistryError) -> Self {
        self.state.lock().unwrap().load_error = Some(error);
        self
    }

    pub fn break_loads(&self, error: RegistryError) {
        self.state.lock().unwrap().load_error = Some(error);
    }

    pub fn mend_loads(&self) {
        self.state.lock().unwrap().load_error = None;
    }

    pub fn sessions(&self) -> Vec<Session> {
        self.state.lock().unwrap().sessions.clone()
    }

    pub fn save_from_elsewhere(&self, session: Session) {
        self.state.lock().unwrap().sessions.push(session);
    }

    pub fn names(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap()
            .sessions
            .iter()
            .map(|session| session.name().to_string())
            .collect()
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
        Ok(PathBuf::from(Self::MOVED_ASIDE_PATH))
    }

    fn moved_aside_copies(&self) -> Result<Vec<PathBuf>, RegistryError> {
        let moved_aside = self.state.lock().unwrap().moved_aside;
        let copies = moved_aside.then(|| PathBuf::from(Self::MOVED_ASIDE_PATH));
        Ok(copies.into_iter().collect())
    }
}
