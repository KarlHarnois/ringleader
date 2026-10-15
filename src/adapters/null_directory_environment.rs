use std::path::Path;

use crate::application::ports::{DirectoryEnvironment, DirectoryEnvironmentError, Envrc};

pub struct NullDirectoryEnvironment;

impl DirectoryEnvironment for NullDirectoryEnvironment {
    fn envrc(&self, _directory: &Path) -> Option<Envrc> {
        None
    }

    fn allow(&self, _directory: &Path) -> Result<(), DirectoryEnvironmentError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_without_direnv_finds_no_envrc() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join(".envrc"), "use flake\n").unwrap();

        assert_eq!(NullDirectoryEnvironment.envrc(directory.path()), None);
    }
}
