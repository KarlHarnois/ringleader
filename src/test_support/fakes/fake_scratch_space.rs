use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::application::ports::{ScratchSpace, ScratchSpaceError};

#[derive(Default)]
pub struct FakeScratchSpace {
    state: Mutex<ScratchState>,
}

#[derive(Default)]
struct ScratchState {
    calls: Vec<(&'static str, PathBuf)>,
    failing: Option<&'static str>,
}

impl FakeScratchSpace {
    const ROOT: &'static str = "/scratch";
    const CREATE: &'static str = "create";
    const REMOVE: &'static str = "remove";

    pub fn failing(self, operation: &'static str) -> Self {
        self.state.lock().unwrap().failing = Some(operation);
        self
    }

    pub fn created(&self) -> Vec<PathBuf> {
        self.calls_of(Self::CREATE)
    }

    pub fn removed(&self) -> Vec<PathBuf> {
        self.calls_of(Self::REMOVE)
    }

    fn calls_of(&self, operation: &str) -> Vec<PathBuf> {
        let state = self.state.lock().unwrap();
        state
            .calls
            .iter()
            .filter(|(called, _)| *called == operation)
            .map(|(_, worktree)| worktree.clone())
            .collect()
    }

    fn record(&self, operation: &'static str, worktree: &Path) -> Result<(), ScratchSpaceError> {
        let mut state = self.state.lock().unwrap();
        if state.failing == Some(operation) {
            return Err(ScratchSpaceError::Io {
                operation,
                path: self.directory(worktree),
                detail: "Permission denied".to_string(),
            });
        }
        state.calls.push((operation, worktree.to_path_buf()));
        Ok(())
    }
}

impl ScratchSpace for FakeScratchSpace {
    fn directory(&self, worktree: &Path) -> PathBuf {
        let name = worktree.file_name().unwrap_or(worktree.as_os_str());
        Path::new(Self::ROOT).join(name)
    }

    fn create(&self, worktree: &Path) -> Result<(), ScratchSpaceError> {
        self.record(Self::CREATE, worktree)
    }

    fn remove(&self, worktree: &Path) -> Result<(), ScratchSpaceError> {
        self.record(Self::REMOVE, worktree)
    }
}
