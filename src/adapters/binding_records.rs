use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::durable_file::DurableFile;
use crate::application::ports::BindingError;

pub struct BindingRecords {
    file: DurableFile,
}

impl BindingRecords {
    pub fn new(path: &Path) -> Self {
        Self {
            file: DurableFile::new(path),
        }
    }

    pub fn path(&self) -> &Path {
        self.file.path()
    }

    pub fn read(&self) -> Result<Vec<BindingRecord>, RecordFileProblem> {
        let text = match fs::read_to_string(self.path()) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(RecordFileProblem::Unreadable(error.to_string())),
        };
        serde_json::from_str::<BindingFile>(&text)
            .map(|file| file.records)
            .map_err(|error| RecordFileProblem::Corrupt(error.to_string()))
    }

    pub fn write(&self, records: Vec<BindingRecord>) -> Result<(), BindingError> {
        let written = if records.is_empty() {
            match fs::remove_file(self.path()) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            }
        } else {
            let text = serde_json::to_string_pretty(&BindingFile { records }).unwrap_or_default();
            self.file.write(text.as_bytes())
        };
        written.map_err(|error| BindingError::Record {
            detail: format!("{}: {error}", self.path().display()),
        })
    }
}

#[derive(Debug)]
pub enum RecordFileProblem {
    Unreadable(String),
    Corrupt(String),
}

#[derive(Serialize, Deserialize)]
struct BindingFile {
    records: Vec<BindingRecord>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct BindingRecord {
    server: ServerIdentity,
    saved: Option<String>,
}

impl BindingRecord {
    pub fn new(server: ServerIdentity, saved: Option<String>) -> Self {
        Self { server, saved }
    }

    pub fn server(&self) -> &ServerIdentity {
        &self.server
    }

    pub fn saved(&self) -> Option<&str> {
        self.saved.as_deref()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerIdentity {
    socket_path: PathBuf,
    pid: String,
}

impl ServerIdentity {
    pub fn new(pid: &str, socket_path: &Path) -> Self {
        Self {
            socket_path: socket_path.to_path_buf(),
            pid: pid.to_string(),
        }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}
