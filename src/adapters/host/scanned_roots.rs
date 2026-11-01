use std::path::{Path, PathBuf};

use crate::adapters::filesystem::host_path::HostPath;

pub struct ScannedRoots {
    roots: Vec<(PathBuf, PathBuf)>,
}

impl ScannedRoots {
    pub fn new(given: &[PathBuf]) -> Self {
        let roots = given
            .iter()
            .map(|root| (root.clone(), HostPath::new(root).canonical_lenient()))
            .collect();
        Self { roots }
    }

    pub fn spelled_from_root(&self, path: &Path) -> Option<PathBuf> {
        self.roots.iter().find_map(|(given, canonical)| {
            path.strip_prefix(given)
                .or_else(|_| path.strip_prefix(canonical))
                .ok()
                .map(|inside| given.join(inside))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RootsFixture {
        base: tempfile::TempDir,
    }

    impl RootsFixture {
        fn new() -> Self {
            let base = tempfile::tempdir().unwrap();
            std::fs::create_dir(base.path().join("real")).unwrap();
            std::os::unix::fs::symlink(base.path().join("real"), base.path().join("link")).unwrap();
            Self { base }
        }

        fn roots(&self) -> ScannedRoots {
            ScannedRoots::new(&[self.base.path().join("link")])
        }

        fn canonical(&self, inside: &str) -> PathBuf {
            HostPath::new(&self.base.path().join("real"))
                .canonical_lenient()
                .join(inside)
        }
    }

    #[test]
    fn a_path_under_a_root_as_given_keeps_its_spelling() {
        let fixture = RootsFixture::new();
        let path = fixture.base.path().join("link/nix-shell.Ab12Cd");

        assert_eq!(fixture.roots().spelled_from_root(&path), Some(path.clone()));
    }

    #[test]
    fn a_canonical_path_is_spelled_from_the_given_root_so_callers_can_compare_it() {
        let fixture = RootsFixture::new();

        let spelled = fixture
            .roots()
            .spelled_from_root(&fixture.canonical("nix-shell.Ab12Cd"));

        assert_eq!(
            spelled,
            Some(fixture.base.path().join("link/nix-shell.Ab12Cd"))
        );
    }

    #[test]
    fn a_path_outside_every_root_is_not_spelled() {
        let fixture = RootsFixture::new();

        assert_eq!(
            fixture
                .roots()
                .spelled_from_root(Path::new("/elsewhere/example")),
            None
        );
    }
}
