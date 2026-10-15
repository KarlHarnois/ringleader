use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::application::Pending;
use crate::application::ServiceError;
use crate::application::ServicePorts;
use crate::application::ports::{ConfigSource, HostProbe, Workspace};
use crate::application::repo_merge::RepoMerge;
use crate::domain::{BaseBranch, Config, Repo, RepoName, Session};

pub struct RepoOverrides {
    workspace: Arc<dyn Workspace>,
    host: Arc<dyn HostProbe>,
    config_source: Arc<dyn ConfigSource>,
}

impl RepoOverrides {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            workspace: ports.repos().workspace(),
            host: ports.repos().host(),
            config_source: ports.state().config_source(),
        }
    }

    pub fn add(&self, request: AddRepoRequest, config: &Config) -> Pending<RepoAddition> {
        let workspace = Arc::clone(&self.workspace);
        let host = Arc::clone(&self.host);
        let config_source = Arc::clone(&self.config_source);
        let explicit = config.repos().to_vec();
        Pending::start(move || {
            let top_level = workspace.validate_repo(&request.path)?;
            if let Some(entry) = RepoMerge::new(host.as_ref()).configured(&top_level, &explicit) {
                return request.against_existing(entry);
            }
            let name = request.name_for(&top_level)?;
            if let Some(entry) = explicit.iter().find(|entry| entry.name() == &name) {
                return Ok(RepoAddition::NameTaken {
                    name,
                    taken_by: entry.clone(),
                });
            }
            let repo = Repo::explicit(name, &top_level, request.base_branch.clone());
            config_source.add_repo(&repo)?;
            Ok(RepoAddition::Added(repo))
        })
    }

    pub fn remove(&self, name: &str, config: &Config, entries: &[Session]) -> Pending<String> {
        let host = Arc::clone(&self.host);
        let config_source = Arc::clone(&self.config_source);
        let entry = config
            .repos()
            .iter()
            .find(|repo| repo.name().as_str() == name)
            .cloned();
        let entries = entries.to_vec();
        let name = name.to_string();
        Pending::start(move || {
            let Some(entry) = entry else {
                return Err(ServiceError::NotFound(format!(
                    "{name} has no [[repos]] entry in {}, a repo found by scanning needs none",
                    config_source.path().display()
                )));
            };
            let override_path = host.canonical_or_given(entry.path());
            let users: Vec<String> = entries
                .iter()
                .filter(|session| host.canonical_or_given(session.repo_path()) == override_path)
                .map(|session| session.name().to_string())
                .collect();
            if !users.is_empty() {
                return Err(ServiceError::Conflict(format!(
                    "sessions {} use {name}, delete them first",
                    users.join(", ")
                )));
            }
            config_source.remove_repo(entry.name())?;
            Ok(name)
        })
    }
}

pub struct AddRepoRequest {
    path: PathBuf,
    name: Option<String>,
    base_branch: Option<BaseBranch>,
}

impl AddRepoRequest {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            name: None,
            base_branch: None,
        }
    }

    pub fn named(self, name: &str) -> Self {
        Self {
            name: Some(name.to_string()),
            ..self
        }
    }

    pub fn based_on(self, base_branch: BaseBranch) -> Self {
        Self {
            base_branch: Some(base_branch),
            ..self
        }
    }

    fn name_for(&self, top_level: &Path) -> Result<RepoName, ServiceError> {
        match &self.name {
            Some(name) => Ok(RepoName::parse(name)?),
            None => Ok(RepoName::of_directory(top_level)?),
        }
    }

    fn against_existing(&self, entry: &Repo) -> Result<RepoAddition, ServiceError> {
        let name_differs = self
            .name
            .as_deref()
            .is_some_and(|name| name != entry.name().as_str());
        let base_differs = self
            .base_branch
            .as_ref()
            .is_some_and(|base| Some(base) != entry.base_branch());
        if name_differs || base_differs {
            return Err(ServiceError::Conflict(format!(
                "{} is already configured as {}, edit that [[repos]] entry instead",
                entry.path().display(),
                entry.name()
            )));
        }
        Ok(RepoAddition::AlreadyConfigured(entry.clone()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoAddition {
    Added(Repo),
    AlreadyConfigured(Repo),
    NameTaken { name: RepoName, taken_by: Repo },
}
