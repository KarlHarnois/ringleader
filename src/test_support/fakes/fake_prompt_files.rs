use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::application::ports::{PromptFileError, PromptFiles};

#[derive(Clone, Default)]
pub struct FakePromptFiles {
    entries: Arc<Mutex<HashMap<PathBuf, Result<String, PromptFileError>>>>,
}

impl FakePromptFiles {
    pub fn holding(self, path: &str, text: &str) -> Self {
        self.with_entry(path, Ok(text.to_string()))
    }

    pub fn failing(self, path: &str, error: PromptFileError) -> Self {
        self.with_entry(path, Err(error))
    }

    fn with_entry(self, path: &str, entry: Result<String, PromptFileError>) -> Self {
        self.entries
            .lock()
            .unwrap()
            .insert(PathBuf::from(path), entry);
        self
    }
}

impl PromptFiles for FakePromptFiles {
    fn read_text(&self, path: &Path) -> Result<String, PromptFileError> {
        self.entries
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .unwrap_or(Err(PromptFileError::Missing))
    }
}
