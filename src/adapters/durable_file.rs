use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

pub struct DurableFile {
    path: PathBuf,
}

impl DurableFile {
    const PRIVATE_MODE: u32 = 0o600;

    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write(&self, contents: &[u8]) -> io::Result<()> {
        let temporary = self.temporary_path();
        let written = self.write_temporary(&temporary, contents);
        if let Err(error) = written.and_then(|()| fs::rename(&temporary, &self.path)) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        self.sync_directory();
        Ok(())
    }

    fn write_temporary(&self, temporary: &Path, contents: &[u8]) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(Self::PRIVATE_MODE)
            .open(temporary)?;
        file.write_all(contents)?;
        file.sync_all()
    }

    fn sync_directory(&self) {
        let directory = self.path.parent().unwrap_or(Path::new("."));
        if let Err(error) = File::open(directory).and_then(|handle| handle.sync_all()) {
            log::warn!("could not sync {}: {error}", directory.display());
        }
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
