use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use super::fnv_hash::FnvHash;
use super::host_path::HostPath;
use super::mount_source::MountSource;
use crate::application::ports::{ScratchSpace, ScratchSpaceError};

pub struct DirectoryScratchSpace {
    root: PathBuf,
    mounts: Box<dyn MountSource>,
}

impl DirectoryScratchSpace {
    const PRIVATE_MODE: u32 = 0o700;
    const OWNER_ACCESS_BITS: u32 = 0o700;

    pub fn new(root: &Path, mounts: Box<dyn MountSource>) -> Self {
        Self {
            root: root.to_path_buf(),
            mounts,
        }
    }

    fn refuse_mounts_within(&self, directory: &Path) -> Result<(), ScratchSpaceError> {
        let canonical = HostPath::new(directory).canonical_lenient();
        let mounts = self
            .mounts
            .mount_table()
            .map_err(|error| Self::failure("check for mounts in", directory, &error))?;
        if mounts.has_mount_within(&canonical) {
            return Err(ScratchSpaceError::HoldsMount {
                path: directory.to_path_buf(),
            });
        }
        Ok(())
    }

    fn remove_tree(directory: &Path) -> io::Result<()> {
        match fs::remove_dir_all(directory) {
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                Self::open_to_owner(directory)?;
                fs::remove_dir_all(directory)
            }
            result => result,
        }
    }

    fn open_to_owner(path: &Path) -> io::Result<()> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_dir() {
            return Ok(());
        }
        let mut permissions = metadata.permissions();
        permissions.set_mode(permissions.mode() | Self::OWNER_ACCESS_BITS);
        fs::set_permissions(path, permissions)?;
        fs::read_dir(path)?.try_for_each(|entry| Self::open_to_owner(&entry?.path()))
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
            .join(FnvHash::of(worktree.as_os_str().as_bytes()).short_hex())
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
        self.refuse_mounts_within(&directory)?;
        match Self::remove_tree(&directory) {
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

    use super::*;
    use crate::adapters::filesystem::mountinfo_file::MountinfoFile;

    struct ScratchFixture {
        base: tempfile::TempDir,
        space: DirectoryScratchSpace,
    }

    impl ScratchFixture {
        const WORKTREE: &'static str = "/data/ringleader/worktrees/example-project/fix-auth";
        const OTHER_WORKTREE: &'static str = "/data/ringleader/worktrees/example-project/fix-ui";
        const WORKTREE_HASH: &'static str = "8718940d";
        const ROOT: &'static str = "cache/ringleader/tmp";
        const MOUNT_TABLE: &'static str = "mountinfo";
        const SEALED_MODE: u32 = 0o500;
        const ACCESS_BITS: u32 = 0o777;

        fn new() -> Self {
            let base = tempfile::tempdir().unwrap();
            let root = base.path().join(Self::ROOT);
            Self::rooted_at(base, &root)
        }

        fn behind_a_symlink() -> Self {
            let base = tempfile::tempdir().unwrap();
            let real = base.path().join("real");
            fs::create_dir(&real).unwrap();
            let link = base.path().join("link");
            std::os::unix::fs::symlink(&real, &link).unwrap();
            let root = link.join(Self::ROOT);
            Self::rooted_at(base, &root)
        }

        fn rooted_at(base: tempfile::TempDir, root: &Path) -> Self {
            let mount_table = base.path().join(Self::MOUNT_TABLE);
            fs::write(&mount_table, "").unwrap();
            let space = DirectoryScratchSpace::new(root, Box::new(MountinfoFile::at(&mount_table)));
            Self { base, space }
        }

        fn without_mount_table(self) -> Self {
            fs::remove_file(self.base.path().join(Self::MOUNT_TABLE)).unwrap();
            self
        }

        fn with_mount_inside(self) -> Self {
            let point = fs::canonicalize(self.scratch()).unwrap().join("mnt");
            let line = format!(
                "41 30 0:40 / {} rw - fuse.sshfs host: rw\n",
                point.display()
            );
            fs::write(self.base.path().join(Self::MOUNT_TABLE), line).unwrap();
            self
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

        fn seal(path: &Path) {
            fs::set_permissions(path, Permissions::from_mode(Self::SEALED_MODE)).unwrap();
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
    fn remove_opens_read_only_directories_to_delete_them() {
        let fixture = ScratchFixture::new().created();
        let module = fixture.scratch().join("gopath/pkg/mod/example");
        fs::create_dir_all(&module).unwrap();
        fs::write(module.join("go.mod"), "module example").unwrap();
        ScratchFixture::seal(&module);
        ScratchFixture::seal(module.parent().unwrap());

        fixture.space.remove(ScratchFixture::worktree()).unwrap();

        assert!(!fixture.scratch().exists());
    }

    #[test]
    fn remove_leaves_the_target_of_a_symlink_alone() {
        let fixture = ScratchFixture::new().created();
        let outside = fixture.base.path().join("outside");
        fs::create_dir(&outside).unwrap();
        ScratchFixture::seal(&outside);
        std::os::unix::fs::symlink(&outside, fixture.scratch().join("link")).unwrap();
        let read_only = fixture.scratch().join("read-only");
        fs::create_dir_all(read_only.join("inner")).unwrap();
        ScratchFixture::seal(&read_only);

        fixture.space.remove(ScratchFixture::worktree()).unwrap();

        assert!(!fixture.scratch().exists());
        assert_eq!(ScratchFixture::mode(&outside), ScratchFixture::SEALED_MODE);
        ScratchFixture::unseal(&outside);
    }

    #[test]
    fn a_root_without_write_access_reports_the_failed_remove() {
        let fixture = ScratchFixture::new().created();
        ScratchFixture::seal(&fixture.root());

        let result = fixture.space.remove(ScratchFixture::worktree());

        ScratchFixture::unseal(&fixture.root());
        assert_eq!(
            ScratchFixture::failed_operation(result),
            Some(("remove", fixture.scratch()))
        );
    }

    #[test]
    fn a_mount_inside_the_directory_keeps_it() {
        let fixture = ScratchFixture::new()
            .created()
            .with_file()
            .with_mount_inside();

        let result = fixture.space.remove(ScratchFixture::worktree());

        assert_eq!(
            result,
            Err(ScratchSpaceError::HoldsMount {
                path: fixture.scratch()
            })
        );
        assert!(fixture.scratch().join("notes.md").exists());
    }

    #[test]
    fn a_mount_listed_under_the_resolved_path_of_a_symlinked_root_keeps_it() {
        let fixture = ScratchFixture::behind_a_symlink()
            .created()
            .with_mount_inside();

        let result = fixture.space.remove(ScratchFixture::worktree());

        assert!(matches!(result, Err(ScratchSpaceError::HoldsMount { .. })));
        assert!(fixture.scratch().exists());
    }

    #[test]
    fn an_unreadable_mount_table_keeps_the_directory() {
        let fixture = ScratchFixture::new().created().without_mount_table();

        let result = fixture.space.remove(ScratchFixture::worktree());

        assert_eq!(
            ScratchFixture::failed_operation(result),
            Some(("check for mounts in", fixture.scratch()))
        );
        assert!(fixture.scratch().exists());
    }
}
