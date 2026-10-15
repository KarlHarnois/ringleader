use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::application::ports::{DirectoryEnvironment, DirectoryEnvironmentError, Envrc};

#[derive(Default)]
pub struct FakeDirectoryEnvironment {
    state: Mutex<EnvrcState>,
}

#[derive(Default)]
struct EnvrcState {
    envrcs: HashMap<PathBuf, Envrc>,
    allowed: Vec<PathBuf>,
    allow_failure: Option<String>,
}

impl FakeDirectoryEnvironment {
    pub fn with_envrc(self, directory: &Path, contents: &str) -> Self {
        let envrc = Self::envrc_of(directory, contents);
        self.storing(directory, envrc)
    }

    pub fn with_allowed_envrc(self, directory: &Path, contents: &str) -> Self {
        let envrc = Self::envrc_of(directory, contents).allowed();
        self.storing(directory, envrc)
    }

    pub fn with_denied_envrc(self, directory: &Path, contents: &str) -> Self {
        let envrc = Self::envrc_of(directory, contents).denied();
        self.storing(directory, envrc)
    }

    pub fn failing_allow(self, reason: &str) -> Self {
        self.state.lock().unwrap().allow_failure = Some(reason.to_string());
        self
    }

    pub fn allowed(&self) -> Vec<PathBuf> {
        self.state.lock().unwrap().allowed.clone()
    }

    fn storing(self, directory: &Path, envrc: Envrc) -> Self {
        let directory = directory.to_path_buf();
        self.state.lock().unwrap().envrcs.insert(directory, envrc);
        self
    }

    fn envrc_of(directory: &Path, contents: &str) -> Envrc {
        let command = format!("direnv allow {}", directory.display());
        Envrc::new(Some(contents.as_bytes().to_vec()), &command)
    }
}

impl DirectoryEnvironment for FakeDirectoryEnvironment {
    fn envrc(&self, directory: &Path) -> Option<Envrc> {
        self.state.lock().unwrap().envrcs.get(directory).cloned()
    }

    fn allow(&self, directory: &Path) -> Result<(), DirectoryEnvironmentError> {
        let mut state = self.state.lock().unwrap();
        if let Some(reason) = &state.allow_failure {
            return Err(DirectoryEnvironmentError::AllowFailed {
                directory: directory.to_path_buf(),
                reason: reason.clone(),
            });
        }
        state.allowed.push(directory.to_path_buf());
        if let Some(envrc) = state.envrcs.remove(directory) {
            state
                .envrcs
                .insert(directory.to_path_buf(), envrc.allowed());
        }
        Ok(())
    }
}
