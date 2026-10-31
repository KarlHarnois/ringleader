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
    directories: Vec<PathBuf>,
    unpurged: Vec<PathBuf>,
}

impl FakeScratchSpace {
    const ROOT: &'static str = "/scratch";
    const CREATE: &'static str = "create";
    const REMOVE: &'static str = "remove";
    const LIST: &'static str = "list";
    const RETIRE: &'static str = "retire";
    const PURGE: &'static str = "purge";

    pub fn failing(self, operation: &'static str) -> Self {
        self.state.lock().unwrap().failing = Some(operation);
        self
    }

    pub fn with_directories_of(self, worktrees: &[PathBuf]) -> Self {
        let directories = worktrees
            .iter()
            .map(|worktree| self.directory(worktree))
            .collect();
        self.state.lock().unwrap().directories = directories;
        self
    }

    pub fn with_unpurged(self, directory: &Path) -> Self {
        self.state
            .lock()
            .unwrap()
            .unpurged
            .push(directory.to_path_buf());
        self
    }

    pub fn retired(&self) -> Vec<PathBuf> {
        self.calls_of(Self::RETIRE)
    }

    pub fn purges(&self) -> usize {
        self.calls_of(Self::PURGE).len()
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
        self.record_at(operation, worktree, &self.directory(worktree))
    }

    fn record_at(
        &self,
        operation: &'static str,
        called_with: &Path,
        directory: &Path,
    ) -> Result<(), ScratchSpaceError> {
        let mut state = self.state.lock().unwrap();
        if state.failing == Some(operation) {
            return Err(ScratchSpaceError::Io {
                operation,
                path: directory.to_path_buf(),
                detail: "Permission denied".to_string(),
            });
        }
        state.calls.push((operation, called_with.to_path_buf()));
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

    fn directories(&self) -> Result<Vec<PathBuf>, ScratchSpaceError> {
        let root = Path::new(Self::ROOT);
        self.record_at(Self::LIST, root, root)?;
        Ok(self.state.lock().unwrap().directories.clone())
    }

    fn retire(&self, directory: &Path) -> Result<(), ScratchSpaceError> {
        self.record_at(Self::RETIRE, directory, directory)
    }

    fn retired_directories(&self) -> Result<Vec<PathBuf>, ScratchSpaceError> {
        Ok(self.state.lock().unwrap().unpurged.clone())
    }

    fn purge_retired(&self) -> Result<(), ScratchSpaceError> {
        let root = Path::new(Self::ROOT);
        self.record_at(Self::PURGE, root, root)
    }
}
