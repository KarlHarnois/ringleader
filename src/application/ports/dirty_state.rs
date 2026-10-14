use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DirtyState {
    missing: bool,
    dirty_files: u64,
    nested_repos: Vec<PathBuf>,
}

impl DirtyState {
    pub fn missing() -> Self {
        Self {
            missing: true,
            ..Self::default()
        }
    }

    pub fn present(dirty_files: u64, nested_repos: Vec<PathBuf>) -> Self {
        Self {
            missing: false,
            dirty_files,
            nested_repos,
        }
    }

    pub fn is_missing(&self) -> bool {
        self.missing
    }

    pub fn dirty_files(&self) -> u64 {
        self.dirty_files
    }

    pub fn nested_repos(&self) -> &[PathBuf] {
        &self.nested_repos
    }
}
