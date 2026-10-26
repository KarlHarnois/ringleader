use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub struct AtomicFile {
    path: PathBuf,
}

impl AtomicFile {
    pub const PRIVATE_MODE: u32 = 0o600;

    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write(&self, contents: &[u8]) -> io::Result<()> {
        self.replace(contents, Self::PRIVATE_MODE, |_| Ok(()))
    }

    pub fn replace(
        &self,
        contents: &[u8],
        mode: u32,
        before_rename: impl FnOnce(&File) -> io::Result<()>,
    ) -> io::Result<()> {
        let temporary = self.temporary_path();
        let replaced = Self::write_temporary(&temporary, contents)
            .and_then(|file| before_rename(&file))
            .and_then(|()| fs::set_permissions(&temporary, fs::Permissions::from_mode(mode)))
            .and_then(|()| fs::rename(&temporary, &self.path));
        if replaced.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        replaced
    }

    fn write_temporary(temporary: &Path, contents: &[u8]) -> io::Result<File> {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(Self::PRIVATE_MODE)
            .open(temporary)?;
        file.write_all(contents)?;
        Ok(file)
    }

    fn temporary_path(&self) -> PathBuf {
        let name = self
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.path
            .with_file_name(format!(".{name}.{}.tmp", std::process::id()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AtomicFileFixture {
        directory: tempfile::TempDir,
    }

    impl AtomicFileFixture {
        const LONGER_THAN_THE_WRITE: usize = 4096;

        fn new() -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
            }
        }

        fn path(&self) -> PathBuf {
            self.directory.path().join("state.json")
        }

        fn entries(&self) -> usize {
            fs::read_dir(self.directory.path()).unwrap().count()
        }
    }

    #[test]
    fn a_write_replaces_the_file_so_a_reader_of_the_old_file_never_sees_a_partial_write() {
        let fixture = AtomicFileFixture::new();
        fs::write(fixture.path(), "old").unwrap();
        let reader_of_the_old_file = fixture.directory.path().join("reader.json");
        fs::hard_link(fixture.path(), &reader_of_the_old_file).unwrap();

        AtomicFile::new(&fixture.path()).write(b"new").unwrap();

        assert_eq!(fs::read_to_string(reader_of_the_old_file).unwrap(), "old");
        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), "new");
    }

    #[test]
    fn a_write_over_a_temporary_file_left_by_a_killed_write_holds_only_the_new_contents() {
        let fixture = AtomicFileFixture::new();
        let leftover = fixture
            .directory
            .path()
            .join(format!(".state.json.{}.tmp", std::process::id()));
        fs::write(
            &leftover,
            "x".repeat(AtomicFileFixture::LONGER_THAN_THE_WRITE),
        )
        .unwrap();

        AtomicFile::new(&fixture.path()).write(b"new").unwrap();

        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), "new");
    }

    #[test]
    fn a_write_that_cannot_rename_leaves_no_temporary_file_behind() {
        let fixture = AtomicFileFixture::new();
        fs::create_dir(fixture.path()).unwrap();

        let written = AtomicFile::new(&fixture.path()).write(b"new");

        assert!(written.is_err());
        assert_eq!(fixture.entries(), 1);
    }

    #[test]
    fn a_failure_before_the_rename_keeps_the_old_contents() {
        let fixture = AtomicFileFixture::new();
        fs::write(fixture.path(), "old").unwrap();

        let replaced =
            AtomicFile::new(&fixture.path()).replace(b"new", AtomicFile::PRIVATE_MODE, |_| {
                Err(io::Error::other("the sync failed"))
            });

        assert!(replaced.is_err());
        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), "old");
        assert_eq!(fixture.entries(), 1);
    }

    #[test]
    fn a_replaced_file_takes_the_requested_mode() {
        const OWNER_READ_WRITE_OTHERS_READ: u32 = 0o644;
        let fixture = AtomicFileFixture::new();

        AtomicFile::new(&fixture.path())
            .replace(b"new", OWNER_READ_WRITE_OTHERS_READ, |_| Ok(()))
            .unwrap();

        let mode = fs::metadata(fixture.path()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, OWNER_READ_WRITE_OTHERS_READ);
    }
}
