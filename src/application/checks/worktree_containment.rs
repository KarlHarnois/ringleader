use std::path::{Component, Path};

use crate::application::ports::HostProbe;

pub struct WorktreeContainment<'a> {
    host: &'a dyn HostProbe,
}

impl<'a> WorktreeContainment<'a> {
    pub fn new(host: &'a dyn HostProbe) -> Self {
        Self { host }
    }

    pub fn contains(&self, root: &Path, worktree: &Path) -> bool {
        if Self::has_parent_component(root) || Self::has_parent_component(worktree) {
            return false;
        }
        let (Ok(root), Ok(worktree)) = (
            self.host.canonicalize_lenient(root),
            self.host.canonicalize_lenient(worktree),
        ) else {
            return false;
        };
        worktree != root && worktree.starts_with(&root)
    }

    pub fn holds(&self, directory: &Path, path: &Path) -> bool {
        let same = self.host.canonical_or_given(directory) == self.host.canonical_or_given(path);
        same || self.contains(directory, path)
    }

    fn has_parent_component(path: &Path) -> bool {
        path.components()
            .any(|component| component == Component::ParentDir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::FakeHostProbe;

    #[test]
    fn containment_rejects_a_parent_directory_component() {
        let host = FakeHostProbe::default();
        let containment = WorktreeContainment::new(&host);

        assert!(!containment.contains(Path::new("/root"), Path::new("/root/../etc")));
    }

    #[test]
    fn containment_rejects_the_root_itself() {
        let host = FakeHostProbe::default();
        let containment = WorktreeContainment::new(&host);

        assert!(!containment.contains(Path::new("/root"), Path::new("/root")));
    }
}
