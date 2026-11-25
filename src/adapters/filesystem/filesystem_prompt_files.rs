use std::fs;
use std::io;
use std::path::Path;

use crate::application::ports::{PromptFileError, PromptFiles};

pub struct FilesystemPromptFiles;

impl PromptFiles for FilesystemPromptFiles {
    fn text(&self, path: &Path) -> Result<String, PromptFileError> {
        match fs::read(path) {
            Ok(bytes) => String::from_utf8(bytes).map_err(|_| PromptFileError::NotText),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Err(PromptFileError::Missing),
            Err(_) => Err(PromptFileError::Unreadable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct PromptFilesFixture {
        directory: tempfile::TempDir,
    }

    impl PromptFilesFixture {
        fn new() -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
            }
        }

        fn path(&self, name: &str) -> std::path::PathBuf {
            self.directory.path().join(name)
        }

        fn files(&self) -> FilesystemPromptFiles {
            FilesystemPromptFiles
        }

        fn writing(&self, name: &str, bytes: &[u8]) -> std::path::PathBuf {
            let path = self.path(name);
            fs::write(&path, bytes).unwrap();
            path
        }
    }

    #[test]
    fn a_text_file_reads_back_unchanged() {
        let fixture = PromptFilesFixture::new();
        let path = fixture.writing(
            "prompt.txt",
            "fix the login\n\nand the quotes \"x\"".as_bytes(),
        );

        assert_eq!(
            fixture.files().text(&path),
            Ok("fix the login\n\nand the quotes \"x\"".to_string())
        );
    }

    #[test]
    fn a_path_with_no_file_is_missing() {
        let fixture = PromptFilesFixture::new();

        assert_eq!(
            fixture.files().text(&fixture.path("absent.txt")),
            Err(PromptFileError::Missing)
        );
    }

    #[test]
    fn a_directory_is_unreadable() {
        let fixture = PromptFilesFixture::new();
        let path = fixture.path("directory");
        fs::create_dir(&path).unwrap();

        assert_eq!(
            fixture.files().text(&path),
            Err(PromptFileError::Unreadable)
        );
    }

    #[test]
    fn bytes_that_are_not_utf8_are_not_text() {
        let fixture = PromptFilesFixture::new();
        let path = fixture.writing("prompt.bin", &[0xff, 0xfe, 0x00]);

        assert_eq!(fixture.files().text(&path), Err(PromptFileError::NotText));
    }
}
