use std::path::Path;

use crate::application::ports::{RepoPageError, RepoPages};

pub struct NullRepoPages;

impl RepoPages for NullRepoPages {
    fn main_page(&self, _repo: &Path) -> Result<Option<String>, RepoPageError> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_repo_has_no_main_page() {
        assert_eq!(
            NullRepoPages.main_page(Path::new("/repos/example-project")),
            Ok(None)
        );
    }
}
