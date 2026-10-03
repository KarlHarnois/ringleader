use std::path::PathBuf;

use thiserror::Error;

use crate::domain::{Config, Repo, RepoName};

pub trait ConfigSource: Send + Sync {
    fn path(&self) -> PathBuf;
    fn load(&self) -> Result<Config, ConfigError>;
    fn add_repo(&self, repo: &Repo) -> Result<(), ConfigError>;
    fn remove_repo(&self, name: &RepoName) -> Result<(), ConfigError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfigError {
    #[error("{path}: {detail}")]
    Parse { path: PathBuf, detail: String },
    #[error("{path}: {}", failures.join("; "))]
    Invalid {
        path: PathBuf,
        failures: Vec<String>,
    },
    #[error("config is Nix-managed, cannot write {path}")]
    NixManaged { path: PathBuf },
    #[error("config is read-only, cannot write {path}")]
    ReadOnly { path: PathBuf },
    #[error("{path} has no [[repos]] entry named {name}")]
    UnknownRepo { path: PathBuf, name: String },
    #[error("cannot access {path}: {detail}")]
    Io { path: PathBuf, detail: String },
}
