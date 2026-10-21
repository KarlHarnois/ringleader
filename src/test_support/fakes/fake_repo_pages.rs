use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::application::ports::{RepoPageError, RepoPages};

#[derive(Default)]
pub struct FakeRepoPages {
    pages: HashMap<PathBuf, String>,
    failure: Option<RepoPageError>,
}

impl FakeRepoPages {
    pub fn with_page(mut self, repo: &Path, page: &str) -> Self {
        self.pages.insert(repo.to_path_buf(), page.to_string());
        self
    }

    pub fn failing(error: RepoPageError) -> Self {
        Self {
            failure: Some(error),
            ..Self::default()
        }
    }
}

impl RepoPages for FakeRepoPages {
    fn main_page(&self, repo: &Path) -> Result<Option<String>, RepoPageError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        Ok(self.pages.get(repo).cloned())
    }
}
