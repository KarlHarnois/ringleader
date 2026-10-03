use std::fs::{self, OpenOptions};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::durable_file::DurableFile;
use crate::application::ports::ConfigError;

pub struct ConfigFileWriter {
    path: PathBuf,
    canonical: PathBuf,
}

impl ConfigFileWriter {
    const NIX_STORE: &'static str = "/nix/store";
    const PERMISSION_BITS: u32 = 0o7777;

    pub fn open(path: &Path) -> Result<Self, ConfigError> {
        let canonical = fs::canonicalize(path).map_err(|error| ConfigError::Io {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;
        let writer = Self {
            path: path.to_path_buf(),
            canonical,
        };
        writer.check_writable()?;
        Ok(writer)
    }

    pub fn text(&self) -> Result<String, ConfigError> {
        fs::read_to_string(&self.canonical).map_err(|error| self.io_error(error))
    }

    pub fn replace(&self, text: &str) -> Result<(), ConfigError> {
        let metadata = fs::metadata(&self.canonical).map_err(|error| self.io_error(error))?;
        let mode = metadata.permissions().mode() & Self::PERMISSION_BITS;
        DurableFile::new(&self.canonical)
            .write_with_mode(text.as_bytes(), mode)
            .map_err(|error| self.io_error(error))
    }

    fn check_writable(&self) -> Result<(), ConfigError> {
        if self.canonical.starts_with(Self::NIX_STORE) {
            return Err(ConfigError::NixManaged {
                path: self.path.clone(),
            });
        }
        match OpenOptions::new().write(true).open(&self.canonical) {
            Ok(_) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                Err(ConfigError::ReadOnly {
                    path: self.path.clone(),
                })
            }
            Err(error) => Err(self.io_error(error)),
        }
    }

    fn io_error(&self, error: io::Error) -> ConfigError {
        ConfigError::Io {
            path: self.path.clone(),
            detail: error.to_string(),
        }
    }
}
