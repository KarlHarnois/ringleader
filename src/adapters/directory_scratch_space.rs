use std::ffi::OsStr;
use std::fs::{self, DirBuilder, Metadata};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};

use super::fnv_hash::FnvHash;
use super::process_environment::ProcessEnvironment;
use super::system_host_probe::SystemHostProbe;
use crate::application::ports::{ScratchSpace, ScratchSpaceError};

pub struct DirectoryScratchSpace {
    root: PathBuf,
    user_id: u32,
}

impl DirectoryScratchSpace {
    const TEMP_VARIABLE: &'static str = "TMPDIR";
    const FALLBACK_TEMP_BASE: &'static str = "/tmp";
    const ROOT_PREFIX: &'static str = "ringleader-";
    const PRIVATE_MODE: u32 = 0o700;
    const GROUP_AND_OTHER_BITS: u32 = 0o077;

    pub fn new(environment: &ProcessEnvironment) -> Self {
        Self::under(&Self::temp_base(environment), SystemHostProbe::user_id())
    }

    fn under(temp_base: &Path, user_id: u32) -> Self {
        Self {
            root: temp_base.join(format!("{}{user_id}", Self::ROOT_PREFIX)),
            user_id,
        }
    }

    fn temp_base(environment: &ProcessEnvironment) -> PathBuf {
        environment
            .get(Self::TEMP_VARIABLE)
            .and_then(OsStr::to_str)
            .map(PathBuf::from)
            .filter(|base| base.is_absolute())
            .unwrap_or_else(|| PathBuf::from(Self::FALLBACK_TEMP_BASE))
    }

    fn refuse_shared_root(&self) -> Result<(), ScratchSpaceError> {
        let metadata = fs::symlink_metadata(&self.root)
            .map_err(|error| Self::failure("inspect", &self.root, &error))?;
        if self.owns_privately(&metadata) {
            return Ok(());
        }
        Err(ScratchSpaceError::NotPrivate {
            path: self.root.clone(),
        })
    }

    fn owns_privately(&self, metadata: &Metadata) -> bool {
        metadata.is_dir()
            && metadata.uid() == self.user_id
            && metadata.mode() & Self::GROUP_AND_OTHER_BITS == 0
    }

    fn root_exists(&self) -> Result<bool, ScratchSpaceError> {
        self.root
            .try_exists()
            .map_err(|error| Self::failure("inspect", &self.root, &error))
    }

    fn make_private_directory(path: &Path) -> Result<(), ScratchSpaceError> {
        match DirBuilder::new().mode(Self::PRIVATE_MODE).create(path) {
            Err(error) if error.kind() != io::ErrorKind::AlreadyExists => {
                Err(Self::failure("create", path, &error))
            }
            _ => Ok(()),
        }
    }

    fn remove_if_present(path: &Path) -> Result<(), ScratchSpaceError> {
        match fs::remove_dir_all(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                Err(Self::failure("remove", path, &error))
            }
            _ => Ok(()),
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
        Self::make_private_directory(&self.root)?;
        self.refuse_shared_root()?;
        Self::make_private_directory(&self.directory(worktree))
    }

    fn remove(&self, worktree: &Path) -> Result<(), ScratchSpaceError> {
        if !self.root_exists()? {
            return Ok(());
        }
        self.refuse_shared_root()?;
        Self::remove_if_present(&self.directory(worktree))
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
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
        const SHARED_MODE: u32 = 0o755;
        const SEALED_MODE: u32 = 0o500;
        const ACCESS_BITS: u32 = 0o777;

        fn new() -> Self {
            let base = tempfile::tempdir().unwrap();
            let space = DirectoryScratchSpace::new(&Self::environment(&[(
                "TMPDIR",
                base.path().to_str().unwrap(),
            )]));
            Self { base, space }
        }

        fn owned_by_another_user() -> Self {
            let base = tempfile::tempdir().unwrap();
            let space = DirectoryScratchSpace::under(base.path(), SystemHostProbe::user_id() + 1);
            Self { base, space }
        }

        fn environment(pairs: &[(&str, &str)]) -> ProcessEnvironment {
            let variables = pairs
                .iter()
                .map(|(name, value)| (OsString::from(name), OsString::from(value)))
                .collect();
            ProcessEnvironment::new(variables, None)
        }

        fn worktree() -> &'static Path {
            Path::new(Self::WORKTREE)
        }

        fn root(&self) -> PathBuf {
            self.base
                .path()
                .join(format!("ringleader-{}", SystemHostProbe::user_id()))
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

        fn shared(self) -> Self {
            fs::set_permissions(self.root(), Permissions::from_mode(Self::SHARED_MODE)).unwrap();
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

        fn is_refused(result: Result<(), ScratchSpaceError>) -> bool {
            matches!(result, Err(ScratchSpaceError::NotPrivate { .. }))
        }
    }

    #[test]
    fn create_makes_a_private_directory_inside_a_private_root() {
        let fixture = ScratchFixture::new().created();

        assert_eq!(ScratchFixture::mode(&fixture.scratch()), 0o700);
        assert_eq!(ScratchFixture::mode(&fixture.root()), 0o700);
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
    fn a_root_open_to_others_refuses_create_and_remove() {
        let fixture = ScratchFixture::new().created().with_file().shared();

        assert!(ScratchFixture::is_refused(
            fixture.space.create(ScratchFixture::worktree())
        ));
        assert!(ScratchFixture::is_refused(
            fixture.space.remove(ScratchFixture::worktree())
        ));
        assert!(fixture.scratch().join("notes.md").exists());
    }

    #[test]
    fn a_root_that_is_a_symlink_is_refused() {
        let fixture = ScratchFixture::new();
        let target = fixture.base.path().join("elsewhere");
        DirBuilder::new().mode(0o700).create(&target).unwrap();
        std::os::unix::fs::symlink(&target, fixture.root()).unwrap();

        assert!(ScratchFixture::is_refused(
            fixture.space.create(ScratchFixture::worktree())
        ));
    }

    #[test]
    fn a_root_owned_by_another_user_is_refused() {
        let fixture = ScratchFixture::owned_by_another_user();

        assert!(ScratchFixture::is_refused(
            fixture.space.create(ScratchFixture::worktree())
        ));
    }

    #[test]
    fn a_root_that_is_a_file_is_refused() {
        let fixture = ScratchFixture::new();
        fs::write(fixture.root(), "not a directory").unwrap();

        assert!(ScratchFixture::is_refused(
            fixture.space.create(ScratchFixture::worktree())
        ));
    }

    #[test]
    fn a_temp_base_without_write_access_reports_the_failed_create() {
        let fixture = ScratchFixture::new();
        let sealed = Permissions::from_mode(ScratchFixture::SEALED_MODE);
        fs::set_permissions(fixture.base.path(), sealed).unwrap();

        let result = fixture.space.create(ScratchFixture::worktree());

        ScratchFixture::unseal(fixture.base.path());
        assert_eq!(
            ScratchFixture::failed_operation(result),
            Some(("create", fixture.root()))
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

    #[test]
    fn a_relative_or_missing_tmpdir_falls_back_to_tmp() {
        let user_root = format!("/tmp/ringleader-{}", SystemHostProbe::user_id());
        let environments = [
            ScratchFixture::environment(&[("TMPDIR", "relative/tmp")]),
            ScratchFixture::environment(&[]),
        ];

        for environment in environments {
            let space = DirectoryScratchSpace::new(&environment);
            let scratch = space.directory(ScratchFixture::worktree());
            assert!(scratch.starts_with(&user_root), "{}", scratch.display());
        }
    }
}
