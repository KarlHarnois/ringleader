use std::fs;
use std::path::{Path, PathBuf};

pub struct HostPath<'a> {
    path: &'a Path,
}

impl<'a> HostPath<'a> {
    pub fn new(path: &'a Path) -> Self {
        Self { path }
    }

    pub fn canonical_lenient(&self) -> PathBuf {
        let existing = self.path.ancestors().find(|ancestor| ancestor.exists());
        let Some(existing) = existing else {
            return self.path.to_path_buf();
        };
        let canonical = fs::canonicalize(existing).unwrap_or_else(|_| existing.to_path_buf());
        match self.path.strip_prefix(existing) {
            Ok(rest) if !rest.as_os_str().is_empty() => canonical.join(rest),
            _ => canonical,
        }
    }

    pub fn subdirectories(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(self.path) else {
            return Vec::new();
        };
        let mut directories: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.path())
            .collect();
        directories.sort();
        directories
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_tail_is_appended_to_the_canonical_existing_ancestor() {
        let directory = tempfile::tempdir().unwrap();
        let canonical = fs::canonicalize(directory.path()).unwrap();
        let missing = directory.path().join("missing/child");

        assert_eq!(
            HostPath::new(&missing).canonical_lenient(),
            canonical.join("missing/child")
        );
    }

    #[test]
    fn a_symlink_in_an_existing_ancestor_is_resolved() {
        let directory = tempfile::tempdir().unwrap();
        let canonical = fs::canonicalize(directory.path()).unwrap();
        fs::create_dir(canonical.join("real")).unwrap();
        std::os::unix::fs::symlink(canonical.join("real"), canonical.join("link")).unwrap();

        assert_eq!(
            HostPath::new(&canonical.join("link/worktree")).canonical_lenient(),
            canonical.join("real/worktree")
        );
    }
}
