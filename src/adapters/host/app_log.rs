use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use env_logger::Target;
use log::LevelFilter;

pub struct AppLog {
    path: PathBuf,
    file: File,
}

impl AppLog {
    const FILE_NAME: &'static str = "app.log";
    pub const MAX_SIZE: u64 = 5 * 1024 * 1024;

    pub fn path_in(state_directory: &Path) -> PathBuf {
        state_directory.join(Self::FILE_NAME)
    }

    pub fn open(state_directory: &Path) -> Result<Self, String> {
        let path = Self::path_in(state_directory);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| Self::disabled(&path, &error.to_string()))?;
        Ok(Self { path, file })
    }

    pub fn disabled(path: &Path, detail: &str) -> String {
        format!("logging disabled: {}: {detail}", path.display())
    }

    pub fn start(&self, level: LevelFilter) -> Result<(), String> {
        let writer = self
            .file
            .try_clone()
            .map_err(|error| Self::disabled(&self.path, &error.to_string()))?;
        let installed = env_logger::Builder::new()
            .filter_level(level)
            .target(Target::Pipe(Box::new(writer)))
            .try_init();
        installed.map_err(|error| Self::disabled(&self.path, &error.to_string()))
    }

    pub fn truncate_if_large(&self) {
        let size = self.file.metadata().map(|metadata| metadata.len());
        if size.is_ok_and(|size| size > Self::MAX_SIZE)
            && let Err(error) = self.file.set_len(0)
        {
            log::warn!("could not truncate {}: {error}", self.path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write;

    use super::*;

    struct LogFixture {
        directory: tempfile::TempDir,
    }

    impl LogFixture {
        fn new() -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
            }
        }

        fn path(&self) -> PathBuf {
            AppLog::path_in(self.directory.path())
        }
    }

    #[test]
    fn opening_appends_to_an_existing_log() {
        let fixture = LogFixture::new();
        fs::write(fixture.path(), "earlier\n").unwrap();

        let log = AppLog::open(fixture.directory.path()).unwrap();
        let mut writer = log.file.try_clone().unwrap();
        writer.write_all(b"later\n").unwrap();

        assert_eq!(
            fs::read_to_string(fixture.path()).unwrap(),
            "earlier\nlater\n"
        );
    }

    #[test]
    fn a_log_over_five_megabytes_is_truncated_in_place() {
        let fixture = LogFixture::new();
        fs::write(fixture.path(), vec![b'x'; 5 * 1024 * 1024 + 1]).unwrap();
        let log = AppLog::open(fixture.directory.path()).unwrap();
        let mut other_writer = OpenOptions::new()
            .append(true)
            .open(fixture.path())
            .unwrap();

        log.truncate_if_large();
        other_writer.write_all(b"after\n").unwrap();

        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), "after\n");
    }

    #[test]
    fn a_small_log_is_kept() {
        let fixture = LogFixture::new();
        fs::write(fixture.path(), "kept\n").unwrap();

        AppLog::open(fixture.directory.path())
            .unwrap()
            .truncate_if_large();

        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), "kept\n");
    }

    #[test]
    fn a_log_that_cannot_be_opened_says_logging_is_disabled() {
        let fixture = LogFixture::new();
        let missing = fixture.directory.path().join("missing");

        let error = AppLog::open(&missing).err().unwrap();

        assert!(error.starts_with(&format!(
            "logging disabled: {}: ",
            AppLog::path_in(&missing).display()
        )));
    }
}
