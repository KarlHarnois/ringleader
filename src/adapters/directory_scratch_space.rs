use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

use super::fnv_hash::FnvHash;
use crate::application::ports::{ScratchSpace, ScratchSpaceError};

pub struct DirectoryScratchSpace {
    root: PathBuf,
}

impl DirectoryScratchSpace {
    const PRIVATE_MODE: u32 = 0o700;

    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    fn failure(operation: &'static str, path: &Path, error: &io::Error) -> ScratchSpaceError {
        ScratchSpaceError::Io {
            operation,
            path: path.to_path_buf(),
            detail: error.to_string(),
        }
    }
}

impl ScratchSpace for DirectoryScratchSpace {
    fn directory(&self, worktree: &Path) -> PathBuf {
        self.root
            .join(FnvHash::of(worktree.as_os_str().as_bytes()).hex())
    }

    fn create(&self, worktree: &Path) -> Result<(), ScratchSpaceError> {
        let directory = self.directory(worktree);
        DirBuilder::new()
            .recursive(true)
            .mode(Self::PRIVATE_MODE)
            .create(&directory)
            .map_err(|error| Self::failure("create", &directory, &error))
    }

    fn remove(&self, worktree: &Path) -> Result<(), ScratchSpaceError> {
        let directory = self.directory(worktree);
        match fs::remove_dir_all(&directory) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                Err(Self::failure("remove", &directory, &error))
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::Permissions;
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    struct ScratchFixture {
        base: tempfile::TempDir,
        space: DirectoryScratchSpace,
    }

    impl ScratchFixture {
        const WORKTREE: &'static str = "/data/ringleader/worktrees/example-project/fix-auth";
        const OTHER_WORKTREE: &'static str = "/data/ringleader/worktrees/example-project/fix-ui";
        const WORKTREE_HASH: &'static str = "779a3f80f082ab8d";
        const ROOT: &'static str = "cache/ringleader/scratch";
        const SEALED_MODE: u32 = 0o500;
        const ACCESS_BITS: u32 = 0o777;

        fn new() -> Self {
            let base = tempfile::tempdir().unwrap();
            let space = DirectoryScratchSpace::new(&base.path().join(Self::ROOT));
            Self { base, space }
        }

        fn worktree() -> &'static Path {
            Path::new(Self::WORKTREE)
        }

        fn root(&self) -> PathBuf {
            self.base.path().join(Self::ROOT)
        }

        fn scratch(&self) -> PathBuf {
            self.space.directory(Self::worktree())
        }

        fn created(self) -> Self {
            self.space.create(Self::worktree()).unwrap();
            self
        }

        fn with_file(self) -> Self {
            fs::write(self.scratch().join("notes.md"), "draft").unwrap();
            self
        }

        fn unseal(path: &Path) {
            fs::set_permissions(
                path,
                Permissions::from_mode(DirectoryScratchSpace::PRIVATE_MODE),
            )
            .unwrap();
        }

        fn failed_operation(
            result: Result<(), ScratchSpaceError>,
        ) -> Option<(&'static str, PathBuf)> {
            match result {
                Err(ScratchSpaceError::Io {
                    operation, path, ..
                }) => Some((operation, path)),
                _ => None,
            }
        }

        fn mode(path: &Path) -> u32 {
            fs::metadata(path).unwrap().permissions().mode() & Self::ACCESS_BITS
        }
    }

    #[test]
    fn create_makes_a_private_directory_and_the_root_above_it() {
        let fixture = ScratchFixture::new().created();

        assert_eq!(ScratchFixture::mode(&fixture.scratch()), 0o700);
        assert!(fixture.root().is_dir());
    }

    #[test]
    fn the_directory_is_named_after_the_hash_of_the_worktree_path() {
        let fixture = ScratchFixture::new();

        assert_eq!(
            fixture.scratch(),
            fixture.root().join(ScratchFixture::WORKTREE_HASH)
        );
    }

    #[test]
    fn another_worktree_gets_another_directory() {
        let fixture = ScratchFixture::new();

        let other = fixture
            .space
            .directory(Path::new(ScratchFixture::OTHER_WORKTREE));

        assert_ne!(other, fixture.scratch());
    }

    #[test]
    fn creating_again_keeps_the_files_inside() {
        let fixture = ScratchFixture::new().created().with_file();

        fixture.space.create(ScratchFixture::worktree()).unwrap();

        assert!(fixture.scratch().join("notes.md").exists());
    }

    #[test]
    fn remove_deletes_the_directory_and_its_files() {
        let fixture = ScratchFixture::new().created().with_file();

        fixture.space.remove(ScratchFixture::worktree()).unwrap();

        assert!(!fixture.scratch().exists());
        assert!(fixture.root().exists());
    }

    #[test]
    fn removing_without_a_root_succeeds() {
        let fixture = ScratchFixture::new();

        fixture.space.remove(ScratchFixture::worktree()).unwrap();

        assert!(!fixture.root().exists());
    }

    #[test]
    fn removing_a_directory_never_created_succeeds() {
        let fixture = ScratchFixture::new().created();
        let never_created = Path::new(ScratchFixture::OTHER_WORKTREE);

        fixture.space.remove(never_created).unwrap();

        assert!(fixture.scratch().exists());
    }

    #[test]
    fn a_file_in_place_of_the_root_reports_the_failed_create() {
        let fixture = ScratchFixture::new();
        fs::create_dir_all(fixture.root().parent().unwrap()).unwrap();
        fs::write(fixture.root(), "not a directory").unwrap();

        let result = fixture.space.create(ScratchFixture::worktree());

        assert_eq!(
            ScratchFixture::failed_operation(result),
            Some(("create", fixture.scratch()))
        );
    }

    #[test]
    fn a_directory_that_cannot_be_emptied_reports_the_failed_remove() {
        let fixture = ScratchFixture::new().created();
        let locked = fixture.scratch().join("locked");
        fs::create_dir(&locked).unwrap();
        fs::write(locked.join("notes.md"), "draft").unwrap();
        fs::set_permissions(&locked, Permissions::from_mode(ScratchFixture::SEALED_MODE)).unwrap();

        let result = fixture.space.remove(ScratchFixture::worktree());

        ScratchFixture::unseal(&locked);
        assert_eq!(
            ScratchFixture::failed_operation(result),
            Some(("remove", fixture.scratch()))
        );
    }
}
