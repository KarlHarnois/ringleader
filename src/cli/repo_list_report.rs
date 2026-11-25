use serde::Serialize;

use super::Reply;
use crate::application::RepoDiscovery;
use crate::domain::Repo;

pub struct RepoListReport {
    discovery: RepoDiscovery,
}

impl RepoListReport {
    pub fn new(discovery: RepoDiscovery) -> Self {
        Self { discovery }
    }

    fn line(repo: &Repo) -> String {
        format!(
            "{:<24} {}{}",
            repo.name(),
            repo.path().display(),
            RepoOrigin::of(repo).marker()
        )
    }

    fn entry(repo: &Repo) -> RepoListEntry {
        RepoListEntry {
            name: repo.name().to_string(),
            path: repo.path().display().to_string(),
            source: RepoOrigin::of(repo).label(),
            base_branch: repo.base_branch().map(|branch| branch.to_string()),
        }
    }
}

impl Reply for RepoListReport {
    fn lines(&self) -> Vec<String> {
        self.discovery.repos().iter().map(Self::line).collect()
    }

    fn notices(&self) -> Vec<String> {
        self.discovery.notices()
    }

    fn result(&self) -> impl Serialize {
        RepoListResult {
            repos: self.discovery.repos().iter().map(Self::entry).collect(),
            notices: self.discovery.notices(),
        }
    }
}

#[derive(Serialize)]
struct RepoListResult {
    repos: Vec<RepoListEntry>,
    notices: Vec<String>,
}

#[derive(Serialize)]
struct RepoListEntry {
    name: String,
    path: String,
    source: &'static str,
    base_branch: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepoOrigin {
    Config,
    Scan,
}

impl RepoOrigin {
    const CONFIG_MARKER: &'static str = "  [[repos]]";
    const SCAN_MARKER: &'static str = "";
    const CONFIG_LABEL: &'static str = "config";
    const SCAN_LABEL: &'static str = "scan";

    fn of(repo: &Repo) -> Self {
        if repo.is_explicit() {
            Self::Config
        } else {
            Self::Scan
        }
    }

    fn marker(self) -> &'static str {
        match self {
            Self::Config => Self::CONFIG_MARKER,
            Self::Scan => Self::SCAN_MARKER,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Config => Self::CONFIG_LABEL,
            Self::Scan => Self::SCAN_LABEL,
        }
    }
}
