use std::path::{Path, PathBuf};

use crate::domain::BaseBranch;
use crate::domain::BuildCacheDir;
use crate::domain::Repo;
use crate::domain::RepoName;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoDraft {
    name: Option<String>,
    path: PathBuf,
    base_branch: Option<String>,
    build_cache_dirs: Vec<String>,
}

impl RepoDraft {
    pub fn new(path: &Path) -> Self {
        Self {
            name: None,
            path: path.to_path_buf(),
            base_branch: None,
            build_cache_dirs: Vec::new(),
        }
    }

    pub fn named(self, name: &str) -> Self {
        Self {
            name: Some(name.to_string()),
            ..self
        }
    }

    pub fn based_on(self, base_branch: &str) -> Self {
        Self {
            base_branch: Some(base_branch.to_string()),
            ..self
        }
    }

    pub fn caching_builds_in(self, build_cache_dirs: &[String]) -> Self {
        Self {
            build_cache_dirs: build_cache_dirs.to_vec(),
            ..self
        }
    }

    fn repo_name(&self) -> Option<RepoName> {
        match &self.name {
            Some(name) => RepoName::parse(name).ok(),
            None => RepoName::of_directory(&self.path).ok(),
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn shown_name(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| self.path.display().to_string())
    }

    pub(super) fn resolved_name(&self) -> Option<RepoName> {
        self.repo_name()
    }

    pub(super) fn unusable_build_cache_dirs(&self) -> impl Iterator<Item = &str> {
        self.build_cache_dirs
            .iter()
            .map(String::as_str)
            .filter(|text| BuildCacheDir::parse(text).is_none())
    }

    pub(super) fn repo(&self) -> Option<Repo> {
        let base_branch = self.base_branch.as_deref().and_then(BaseBranch::new);
        let build_cache_dirs = self
            .build_cache_dirs
            .iter()
            .filter_map(|text| BuildCacheDir::parse(text))
            .collect();
        Some(
            Repo::explicit(self.repo_name()?, &self.path, base_branch)
                .caching_builds_in(build_cache_dirs),
        )
    }
}
