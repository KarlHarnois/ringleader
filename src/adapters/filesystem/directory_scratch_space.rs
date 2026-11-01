use std::ffi::OsStr;
use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::fnv_hash::FnvHash;
use super::host_path::HostPath;
use super::mount_source::MountSource;
use super::tool_temp_name::ToolTempName;
use crate::application::ports::{ScratchSpace, ScratchSpaceError, ToolTempDirectory};

pub struct DirectoryScratchSpace {
    root: PathBuf,
    mounts: Box<dyn MountSource>,
}

impl DirectoryScratchSpace {
    const PRIVATE_MODE: u32 = 0o700;
    const OWNER_ACCESS_BITS: u32 = 0o700;
    const RETIRED: &'static str = ".retired";

    pub fn new(root: &Path, mounts: Box<dyn MountSource>) -> Self {
        Self {
            root: root.to_path_buf(),
            mounts,
        }
    }

    fn remove_directory(&self, directory: &Path) -> Result<(), ScratchSpaceError> {
        self.refuse_mounts_within(directory)?;
        match Self::remove_tree(directory) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                Err(Self::failure("remove", directory, &error))
            }
            _ => Ok(()),
        }
    }

    fn entries_of(directory: &Path) -> io::Result<Vec<PathBuf>> {
        match fs::read_dir(directory) {
            Ok(entries) => entries.map(|entry| Ok(entry?.path())).collect(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error),
        }
    }

    fn refuse_foreign(&self, directory: &Path) -> Result<(), ScratchSpaceError> {
        if self.is_scratch_directory(directory) {
            return Ok(());
        }
        Err(ScratchSpaceError::Foreign {
            path: directory.to_path_buf(),
        })
    }

    fn refuse_unrecognized_tool_temp(&self, path: &Path) -> Result<(), ScratchSpaceError> {
        let in_scratch = path
            .parent()
            .is_some_and(|parent| self.is_scratch_directory(parent));
        if in_scratch && Self::is_tool_temp(path) {
            return Ok(());
        }
        Err(ScratchSpaceError::NotToolTemp {
            path: path.to_path_buf(),
        })
    }

    fn is_tool_temp(path: &Path) -> bool {
        Self::is_real_directory(path) && ToolTempName::recognize(path).is_some()
    }

    fn refuse_unretired(&self, path: &Path) -> Result<(), ScratchSpaceError> {
        self.refuse_linked_retired_root()?;
        let named = path.file_name().is_some();
        if named && path.parent() == Some(self.retired_root().as_path()) {
            return Ok(());
        }
        Err(ScratchSpaceError::NotRetired {
            path: path.to_path_buf(),
        })
    }

    fn refuse_linked_retired_root(&self) -> Result<(), ScratchSpaceError> {
        let retired = self.retired_root();
        let real_or_absent = match fs::symlink_metadata(&retired) {
            Ok(metadata) => metadata.is_dir(),
            Err(error) => error.kind() == io::ErrorKind::NotFound,
        };
        if real_or_absent {
            return Ok(());
        }
        Err(ScratchSpaceError::LinkedRetiredRoot { path: retired })
    }

    fn is_scratch_directory(&self, path: &Path) -> bool {
        let inside_root = path.parent() == Some(self.root.as_path());
        let hash_named = path
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(FnvHash::is_short_hex);
        inside_root && hash_named && Self::is_real_directory(path)
    }

    fn is_real_directory(path: &Path) -> bool {
        fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
    }

    fn tool_temp(entry: &Path) -> Option<ToolTempDirectory> {
        let metadata = fs::symlink_metadata(entry)
            .ok()
            .filter(fs::Metadata::is_dir)?;
        let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
        Some(ToolTempName::recognize(entry)?.modified_at(modified.as_secs()))
    }

    fn move_into_retired(&self, directory: &Path) -> io::Result<()> {
        DirBuilder::new()
            .recursive(true)
            .mode(Self::PRIVATE_MODE)
            .create(self.retired_root())?;
        let destination = self.unused_retired_path(directory);
        match fs::rename(directory, &destination) {
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                Self::grant_owner_access(directory)?;
                fs::rename(directory, &destination)
            }
            result => result,
        }
    }

    fn unused_retired_path(&self, directory: &Path) -> PathBuf {
        let name = directory.file_name().unwrap_or_default().to_string_lossy();
        let mut attempt = 0;
        loop {
            let candidate = self.retired_root().join(format!("{name}-{attempt}"));
            if !candidate.exists() {
                return candidate;
            }
            attempt += 1;
        }
    }

    fn retired_root(&self) -> PathBuf {
        self.root.join(Self::RETIRED)
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
        Self::grant_owner_access(path)?;
        fs::read_dir(path)?.try_for_each(|entry| Self::open_to_owner(&entry?.path()))
    }

    fn grant_owner_access(path: &Path) -> io::Result<()> {
        let mut permissions = fs::symlink_metadata(path)?.permissions();
        permissions.set_mode(permissions.mode() | Self::OWNER_ACCESS_BITS);
        fs::set_permissions(path, permissions)
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
        self.remove_directory(&self.directory(worktree))
    }

    fn directories(&self) -> Result<Vec<PathBuf>, ScratchSpaceError> {
        let entries = Self::entries_of(&self.root)
            .map_err(|error| Self::failure("list", &self.root, &error))?;
        Ok(entries
            .into_iter()
            .filter(|entry| self.is_scratch_directory(entry))
            .collect())
    }

    fn retire(&self, directory: &Path) -> Result<(), ScratchSpaceError> {
        self.refuse_foreign(directory)?;
        self.refuse_linked_retired_root()?;
        self.refuse_mounts_within(directory)?;
        self.move_into_retired(directory)
            .map_err(|error| Self::failure("retire", directory, &error))
    }

    fn retired_directories(&self) -> Result<Vec<PathBuf>, ScratchSpaceError> {
        self.refuse_linked_retired_root()?;
        let retired = self.retired_root();
        Self::entries_of(&retired).map_err(|error| Self::failure("list", &retired, &error))
    }

    fn purge(&self, retired: &Path) -> Result<(), ScratchSpaceError> {
        self.refuse_unretired(retired)?;
        self.remove_directory(retired)
    }

    fn tool_temp_directories(
        &self,
        directory: &Path,
    ) -> Result<Vec<ToolTempDirectory>, ScratchSpaceError> {
        let entries = Self::entries_of(directory)
            .map_err(|error| Self::failure("list", directory, &error))?;
        Ok(entries
            .iter()
            .filter_map(|entry| Self::tool_temp(entry))
            .collect())
    }

    fn retire_tool_temp(&self, tool_temp: &Path) -> Result<(), ScratchSpaceError> {
        self.refuse_unrecognized_tool_temp(tool_temp)?;
        self.refuse_linked_retired_root()?;
        self.refuse_mounts_within(tool_temp)?;
        self.move_into_retired(tool_temp)
            .map_err(|error| Self::failure("retire", tool_temp, &error))
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
        const UNREADABLE_MODE: u32 = 0o300;
        const ACCESS_BITS: u32 = 0o777;
        const SHELL: &'static str = "nix-shell.Ab12Cd";
        const LAUNCHER: &'static str = "nix-develop-4100-7";
        const LAUNCHER_OWNER: u32 = 4100;

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
            let scratch = self.scratch();
            self.with_mount_in(&scratch)
        }

        fn with_mount_in(self, directory: &Path) -> Self {
            let point = fs::canonicalize(directory).unwrap().join("mnt");
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

        fn retired(&self) -> PathBuf {
            self.root().join(DirectoryScratchSpace::RETIRED)
        }

        fn linked_retired_root(&self) -> PathBuf {
            let target = self.base.path().join("example-target");
            fs::create_dir(&target).unwrap();
            std::os::unix::fs::symlink(&target, self.retired()).unwrap();
            target
        }

        fn retired_scratch(&self) -> PathBuf {
            self.retired().join(format!("{}-0", Self::WORKTREE_HASH))
        }

        fn retired_count(&self) -> usize {
            fs::read_dir(self.retired()).unwrap().count()
        }

        fn retire_created(&self) {
            self.retire_scratch_of(Self::worktree());
        }

        fn retire_scratch_of(&self, worktree: &Path) {
            self.space.create(worktree).unwrap();
            self.space.retire(&self.space.directory(worktree)).unwrap();
        }

        fn with_tool_temps(self) -> Self {
            let scratch = self.scratch();
            for directory in [Self::SHELL, Self::LAUNCHER, "claude-1000"] {
                fs::create_dir(scratch.join(directory)).unwrap();
            }
            fs::write(scratch.join("nix-shell.Ef34Gh"), "not a folder").unwrap();
            std::os::unix::fs::symlink(self.base.path(), scratch.join("nix-shell.Gh56Ij")).unwrap();
            self
        }

        fn tool_temp(&self, name: &str) -> PathBuf {
            self.scratch().join(name)
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

    #[test]
    fn directories_lists_each_created_directory() {
        let fixture = ScratchFixture::new().created();

        assert_eq!(fixture.space.directories().unwrap(), [fixture.scratch()]);
    }

    #[test]
    fn directories_skip_entries_that_ringleader_did_not_make() {
        let fixture = ScratchFixture::new().created();
        fs::create_dir(fixture.root().join("example-cache")).unwrap();
        fs::write(fixture.root().join("0123abcd"), "not a directory").unwrap();
        std::os::unix::fs::symlink(fixture.scratch(), fixture.root().join("4567cdef")).unwrap();

        assert_eq!(fixture.space.directories().unwrap(), [fixture.scratch()]);
    }

    #[test]
    fn directories_without_a_root_lists_nothing() {
        let fixture = ScratchFixture::new();

        assert!(fixture.space.directories().unwrap().is_empty());
    }

    #[test]
    fn retire_takes_the_directory_out_of_the_listing() {
        let fixture = ScratchFixture::new().created().with_file();

        fixture.space.retire(&fixture.scratch()).unwrap();

        assert!(!fixture.scratch().exists());
        assert!(fixture.space.directories().unwrap().is_empty());
        assert_eq!(fixture.retired_count(), 1);
    }

    #[test]
    fn retiring_a_recreated_directory_keeps_the_one_retired_before() {
        let fixture = ScratchFixture::new();
        fixture.retire_created();

        fixture.retire_created();

        assert_eq!(fixture.retired_count(), 2);
    }

    #[test]
    fn retire_opens_a_sealed_scratch_directory_since_a_move_rewrites_its_parent_link() {
        let fixture = ScratchFixture::new().created();
        ScratchFixture::seal(&fixture.scratch());

        fixture.space.retire(&fixture.scratch()).unwrap();

        assert!(!fixture.scratch().exists());
        assert_eq!(fixture.retired_count(), 1);
    }

    #[test]
    fn retire_refuses_a_path_that_is_not_a_scratch_directory() {
        let fixture = ScratchFixture::new().created();
        let outside = fixture.base.path().join("outside");
        fs::create_dir(&outside).unwrap();

        let result = fixture.space.retire(&outside);

        assert_eq!(
            result,
            Err(ScratchSpaceError::Foreign {
                path: outside.clone()
            })
        );
        assert!(outside.exists());
    }

    #[test]
    fn retire_refuses_a_hash_named_directory_outside_the_root_so_a_purge_cannot_reach_it() {
        let fixture = ScratchFixture::new().created();
        let outside = fixture.base.path().join(ScratchFixture::WORKTREE_HASH);
        fs::create_dir(&outside).unwrap();

        let result = fixture.space.retire(&outside);

        assert_eq!(
            result,
            Err(ScratchSpaceError::Foreign {
                path: outside.clone()
            })
        );
        assert!(outside.exists());
    }

    #[test]
    fn retire_keeps_a_directory_that_holds_a_mount() {
        let fixture = ScratchFixture::new().created().with_mount_inside();

        let result = fixture.space.retire(&fixture.scratch());

        assert!(matches!(result, Err(ScratchSpaceError::HoldsMount { .. })));
        assert!(fixture.scratch().exists());
    }

    #[test]
    fn a_symlinked_retired_root_is_never_listed_or_purged_so_its_target_stays() {
        let fixture = ScratchFixture::new().created();
        let target = fixture.linked_retired_root();
        let kept = target.join("keep-me");
        fs::create_dir(&kept).unwrap();

        let listing = fixture.space.retired_directories();
        let purge = fixture.space.purge(&fixture.retired().join("keep-me"));

        assert!(matches!(
            listing,
            Err(ScratchSpaceError::LinkedRetiredRoot { .. })
        ));
        assert!(matches!(
            purge,
            Err(ScratchSpaceError::LinkedRetiredRoot { .. })
        ));
        assert!(kept.exists());
    }

    #[test]
    fn retire_refuses_a_symlinked_retired_root_so_nothing_moves_into_its_target() {
        let fixture = ScratchFixture::new().created();
        let target = fixture.linked_retired_root();

        let result = fixture.space.retire(&fixture.scratch());

        assert!(matches!(
            result,
            Err(ScratchSpaceError::LinkedRetiredRoot { .. })
        ));
        assert!(fixture.scratch().exists());
        assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
    }

    #[test]
    fn purge_removes_a_retired_directory() {
        let fixture = ScratchFixture::new();
        fixture.retire_created();

        fixture.space.purge(&fixture.retired_scratch()).unwrap();

        assert_eq!(fixture.retired_count(), 0);
    }

    #[test]
    fn purge_refuses_a_directory_that_was_never_retired() {
        let fixture = ScratchFixture::new().created();

        let result = fixture.space.purge(&fixture.scratch());

        assert_eq!(
            result,
            Err(ScratchSpaceError::NotRetired {
                path: fixture.scratch()
            })
        );
        assert!(fixture.scratch().exists());
    }

    #[test]
    fn purge_refuses_the_parent_of_the_retired_root_so_live_directories_stay() {
        let fixture = ScratchFixture::new();
        fixture.retire_created();
        fixture
            .space
            .create(Path::new(ScratchFixture::OTHER_WORKTREE))
            .unwrap();
        let parent = fixture.retired().join("..");

        let result = fixture.space.purge(&parent);

        assert!(matches!(result, Err(ScratchSpaceError::NotRetired { .. })));
        assert!(
            fixture
                .space
                .directory(Path::new(ScratchFixture::OTHER_WORKTREE))
                .exists()
        );
    }

    #[test]
    fn purge_keeps_a_retired_directory_that_holds_a_mount() {
        let fixture = ScratchFixture::new();
        fixture.retire_created();
        let mounted = fixture.retired_scratch();
        let fixture = fixture.with_mount_in(&mounted);

        let result = fixture.space.purge(&mounted);

        assert!(matches!(result, Err(ScratchSpaceError::HoldsMount { .. })));
        assert!(mounted.exists());
    }

    #[test]
    fn an_unreadable_mount_table_keeps_a_retired_directory() {
        let fixture = ScratchFixture::new();
        fixture.retire_created();
        let fixture = fixture.without_mount_table();

        let result = fixture.space.purge(&fixture.retired_scratch());

        assert!(result.is_err());
        assert_eq!(fixture.retired_count(), 1);
    }

    #[test]
    fn no_retired_root_lists_no_retired_directory() {
        let fixture = ScratchFixture::new().created();

        assert!(fixture.space.retired_directories().unwrap().is_empty());
    }

    #[test]
    fn an_unreadable_retired_root_fails_the_listing() {
        let fixture = ScratchFixture::new();
        fixture.retire_created();
        let retired = fixture.retired();
        fs::set_permissions(
            &retired,
            Permissions::from_mode(ScratchFixture::UNREADABLE_MODE),
        )
        .unwrap();

        let listing = fixture.space.retired_directories();

        ScratchFixture::unseal(&retired);
        assert!(matches!(listing, Err(ScratchSpaceError::Io { .. })));
    }

    #[test]
    fn the_tool_temp_directories_are_the_real_nix_folders_and_no_other_entry() {
        let fixture = ScratchFixture::new().created().with_tool_temps();

        let mut listed = fixture
            .space
            .tool_temp_directories(&fixture.scratch())
            .unwrap();

        listed.sort_by(|first, second| first.path().cmp(second.path()));
        let named: Vec<(&Path, Option<u32>)> = listed
            .iter()
            .map(|tool_temp| (tool_temp.path(), tool_temp.owner()))
            .collect();
        assert_eq!(
            named,
            [
                (
                    fixture.tool_temp(ScratchFixture::LAUNCHER).as_path(),
                    Some(ScratchFixture::LAUNCHER_OWNER)
                ),
                (fixture.tool_temp(ScratchFixture::SHELL).as_path(), None),
            ]
        );
    }

    #[test]
    fn a_tool_temp_directory_carries_its_modification_time() {
        let fixture = ScratchFixture::new().created().with_tool_temps();
        let shell = fixture.tool_temp(ScratchFixture::SHELL);
        let modified = fs::metadata(&shell).unwrap().modified().unwrap();

        let listed = fixture
            .space
            .tool_temp_directories(&fixture.scratch())
            .unwrap();

        let listed_shell = listed
            .iter()
            .find(|tool_temp| tool_temp.path() == shell)
            .unwrap();
        assert_eq!(
            listed_shell.modified_epoch_seconds(),
            modified.duration_since(UNIX_EPOCH).unwrap().as_secs()
        );
    }

    #[test]
    fn a_missing_scratch_directory_holds_no_tool_temp_directory() {
        let fixture = ScratchFixture::new();

        let listed = fixture.space.tool_temp_directories(&fixture.scratch());

        assert_eq!(listed, Ok(Vec::new()));
    }

    #[test]
    fn retiring_a_tool_temp_directory_moves_it_into_the_retired_root() {
        let fixture = ScratchFixture::new().created().with_tool_temps();
        let shell = fixture.tool_temp(ScratchFixture::SHELL);

        fixture.space.retire_tool_temp(&shell).unwrap();

        assert!(!shell.exists());
        assert!(
            fixture
                .retired()
                .join(format!("{}-0", ScratchFixture::SHELL))
                .is_dir()
        );
    }

    #[test]
    fn retiring_keeps_an_entry_that_is_not_a_tool_temp_directory() {
        let fixture = ScratchFixture::new().created().with_tool_temps();
        let other = fixture.tool_temp("claude-1000");

        let result = fixture.space.retire_tool_temp(&other);

        assert_eq!(
            result,
            Err(ScratchSpaceError::NotToolTemp {
                path: other.clone()
            })
        );
        assert!(other.is_dir());
    }

    #[test]
    fn retiring_keeps_a_nix_folder_outside_every_scratch_directory() {
        let fixture = ScratchFixture::new().created();
        let outside = fixture.base.path().join(ScratchFixture::SHELL);
        fs::create_dir(&outside).unwrap();

        let result = fixture.space.retire_tool_temp(&outside);

        assert!(matches!(result, Err(ScratchSpaceError::NotToolTemp { .. })));
        assert!(outside.is_dir());
    }

    #[test]
    fn retiring_keeps_a_tool_temp_directory_that_holds_a_mount() {
        let fixture = ScratchFixture::new().created().with_tool_temps();
        let shell = fixture.tool_temp(ScratchFixture::SHELL);
        let fixture = fixture.with_mount_in(&shell);

        let result = fixture.space.retire_tool_temp(&shell);

        assert!(matches!(result, Err(ScratchSpaceError::HoldsMount { .. })));
        assert!(shell.is_dir());
    }
}
