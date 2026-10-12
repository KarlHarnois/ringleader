use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::application::ports::{LeftoverBinding, LeftoverBindings, LeftoverBindingsError};

pub struct LeftoverBindingsFile {
    path: PathBuf,
}

impl LeftoverBindingsFile {
    pub const FILE_NAME: &'static str = "bindings.json";

    pub fn new(state_directory: &Path) -> Self {
        Self {
            path: state_directory.join(Self::FILE_NAME),
        }
    }

    fn parse(&self, text: &str) -> Result<Vec<LeftoverBinding>, LeftoverBindingsError> {
        let file: BindingFile =
            serde_json::from_str(text).map_err(|error| LeftoverBindingsError::Corrupt {
                path: self.path.clone(),
                detail: error.to_string(),
            })?;
        Ok(file.records.iter().map(BindingRecord::binding).collect())
    }
}

impl LeftoverBindings for LeftoverBindingsFile {
    fn path(&self) -> PathBuf {
        self.path.clone()
    }

    fn read(&self) -> Result<Vec<LeftoverBinding>, LeftoverBindingsError> {
        match fs::read_to_string(&self.path) {
            Ok(text) => self.parse(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(LeftoverBindingsError::Unreadable {
                path: self.path.clone(),
                detail: error.to_string(),
            }),
        }
    }
}

#[derive(Deserialize)]
struct BindingFile {
    records: Vec<BindingRecord>,
}

#[derive(Deserialize)]
struct BindingRecord {
    server: ServerIdentity,
    saved: Option<String>,
}

impl BindingRecord {
    fn binding(&self) -> LeftoverBinding {
        LeftoverBinding::new(&self.server.socket_path, self.saved.as_deref())
    }
}

#[derive(Deserialize)]
struct ServerIdentity {
    socket_path: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../tests/fixtures/bindings.json");

    struct FileFixture {
        directory: tempfile::TempDir,
    }

    impl FileFixture {
        fn with_text(text: &str) -> Self {
            let directory = tempfile::tempdir().unwrap();
            fs::write(directory.path().join(LeftoverBindingsFile::FILE_NAME), text).unwrap();
            Self { directory }
        }

        fn read(&self) -> Result<Vec<LeftoverBinding>, LeftoverBindingsError> {
            LeftoverBindingsFile::new(self.directory.path()).read()
        }
    }

    #[test]
    fn each_record_gives_its_socket_and_saved_line() {
        let bindings = FileFixture::with_text(FIXTURE).read().unwrap();

        assert_eq!(
            bindings,
            [
                LeftoverBinding::new(
                    Path::new("/tmp/tmux-1000/default"),
                    Some("bind-key -T root C-q send-keys example")
                ),
                LeftoverBinding::new(Path::new("/tmp/tmux-1000/work"), None),
            ]
        );
    }

    #[test]
    fn a_missing_file_holds_no_bindings() {
        let directory = tempfile::tempdir().unwrap();

        let bindings = LeftoverBindingsFile::new(directory.path()).read().unwrap();

        assert!(bindings.is_empty());
    }

    #[test]
    fn invalid_json_is_reported_with_the_path() {
        let error = FileFixture::with_text("{").read().unwrap_err();

        assert!(matches!(error, LeftoverBindingsError::Corrupt { .. }));
        assert!(error.to_string().contains(LeftoverBindingsFile::FILE_NAME));
    }
}
