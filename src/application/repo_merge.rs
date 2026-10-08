use std::path::Path;

use super::ports::HostProbe;
use crate::domain::{InvalidName, Repo, RepoName, RepoScope};

pub struct RepoMerge<'a> {
    host: &'a dyn HostProbe,
}

impl<'a> RepoMerge<'a> {
    pub fn new(host: &'a dyn HostProbe) -> Self {
        Self { host }
    }

    pub fn merge(&self, scope: &RepoScope, scanned: &[Repo], explicit: &[Repo]) -> Vec<Repo> {
        let mut merged: Vec<Repo> = explicit.iter().map(|repo| self.canonical(repo)).collect();
        let enclosing = scope
            .enclosing_repo()
            .and_then(|top_level| Self::discovered(top_level).ok());
        for repo in enclosing.iter().chain(scanned) {
            if merged.iter().all(|kept| kept.path() != repo.path()) {
                merged.push(repo.clone());
            }
        }
        merged.sort_by(|first, second| {
            (first.name(), first.path()).cmp(&(second.name(), second.path()))
        });
        merged
    }

    pub fn resolved(&self, top_level: &Path, explicit: &[Repo]) -> Result<Repo, InvalidName> {
        let top_level = self
            .host
            .canonicalize_lenient(top_level)
            .unwrap_or_else(|_| top_level.to_path_buf());
        let configured = explicit
            .iter()
            .map(|repo| self.canonical(repo))
            .find(|repo| repo.path() == top_level);
        match configured {
            Some(repo) => Ok(repo),
            None => Self::discovered(&top_level),
        }
    }

    fn canonical(&self, repo: &Repo) -> Repo {
        let canonical = self
            .host
            .canonicalize_lenient(repo.path())
            .unwrap_or_else(|_| repo.path().to_path_buf());
        repo.at_path(&canonical)
    }

    fn discovered(top_level: &Path) -> Result<Repo, InvalidName> {
        let basename = top_level
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Repo::discovered(
            RepoName::from_basename(&basename)?,
            top_level,
        ))
    }
}
