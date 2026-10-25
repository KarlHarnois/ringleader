use std::fs::File;
use std::io;
use std::path::Path;

use super::atomic_file::AtomicFile;

pub struct DurableFile {
    file: AtomicFile,
}

impl DurableFile {
    pub fn new(path: &Path) -> Self {
        Self {
            file: AtomicFile::new(path),
        }
    }

    pub fn path(&self) -> &Path {
        self.file.path()
    }

    pub fn write(&self, contents: &[u8]) -> io::Result<()> {
        self.write_with_mode(contents, AtomicFile::PRIVATE_MODE)
    }

    pub fn write_with_mode(&self, contents: &[u8], mode: u32) -> io::Result<()> {
        self.file.replace(contents, mode, File::sync_all)?;
        self.sync_directory();
        Ok(())
    }

    fn sync_directory(&self) {
        let directory = self.path().parent().unwrap_or(Path::new("."));
        if let Err(error) = File::open(directory).and_then(|handle| handle.sync_all()) {
            log::warn!("could not sync {}: {error}", directory.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn a_write_replaces_the_contents_and_leaves_no_temporary_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        fs::write(&path, "old").unwrap();

        DurableFile::new(&path).write(b"new").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_failed_write_keeps_the_old_contents() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("missing-dir/state.json");

        assert!(DurableFile::new(&path).write(b"new").is_err());
    }
}
