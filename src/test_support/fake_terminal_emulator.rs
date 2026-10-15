use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::application::ports::{TerminalEmulator, TerminalEmulatorError};

#[derive(Default)]
pub struct FakeTerminalEmulator {
    opened: Mutex<Vec<PathBuf>>,
    failure: Option<TerminalEmulatorError>,
}

impl FakeTerminalEmulator {
    pub fn failing(error: TerminalEmulatorError) -> Self {
        Self {
            failure: Some(error),
            ..Self::default()
        }
    }

    pub fn opened(&self) -> Vec<PathBuf> {
        self.opened.lock().unwrap().clone()
    }
}

impl TerminalEmulator for FakeTerminalEmulator {
    fn open_window(&self, directory: &Path) -> Result<(), TerminalEmulatorError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        self.opened.lock().unwrap().push(directory.to_path_buf());
        Ok(())
    }
}
