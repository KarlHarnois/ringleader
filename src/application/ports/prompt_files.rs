use std::path::Path;

pub trait PromptFiles {
    fn read_text(&self, path: &Path) -> Result<String, PromptFileError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptFileError {
    Missing,
    Unreadable,
    NotText,
}
