use std::fmt;
use std::path::{Path, PathBuf};

use crate::application::{Pending, RepoDiscovery, SessionService};
use crate::domain::Repo;

pub struct RepoScan {
    origin: ScanOrigin,
    state: ScanState,
}

impl RepoScan {
    pub fn start(service: &SessionService, scope: &Path) -> Self {
        Self {
            origin: ScanOrigin::Scope(scope.to_path_buf()),
            state: ScanState::Scanning(service.discover_repos(scope)),
        }
    }

    pub fn home(service: &SessionService) -> Self {
        Self {
            origin: ScanOrigin::Home,
            state: ScanState::Scanning(service.find_home_repos()),
        }
    }

    pub fn again(&self, service: &SessionService) -> Self {
        match &self.origin {
            ScanOrigin::Scope(scope) => Self::start(service, scope),
            ScanOrigin::Home => Self::home(service),
        }
    }

    pub fn finish_if_done(&mut self) -> bool {
        let ScanState::Scanning(pending) = &self.state else {
            return false;
        };
        let Some(result) = pending.poll() else {
            return false;
        };
        self.state = match result {
            Ok(found) => ScanState::Found(found),
            Err(error) => ScanState::Failed(error.to_string()),
        };
        true
    }

    pub fn is_scanning(&self) -> bool {
        matches!(self.state, ScanState::Scanning(_))
    }

    pub fn found(&self) -> Option<&RepoDiscovery> {
        match &self.state {
            ScanState::Found(found) => Some(found),
            _ => None,
        }
    }

    pub fn repos(&self) -> &[Repo] {
        self.found().map_or(&[], RepoDiscovery::repos)
    }

    pub fn header(&self) -> String {
        match &self.state {
            ScanState::Scanning(_) => format!("Scanning {}", self.origin),
            ScanState::Found(found) => format!("Scope: {}", found.scope().root().display()),
            ScanState::Failed(_) => format!("Scope: {}", self.origin),
        }
    }

    pub fn notices(&self) -> Vec<String> {
        match &self.state {
            ScanState::Found(found) => found.notices(),
            _ => Vec::new(),
        }
    }

    pub fn problem(&self) -> Option<String> {
        match &self.state {
            ScanState::Found(found) if found.repos().is_empty() => Some(format!(
                "no repos found in {}",
                found.scope().root().display()
            )),
            ScanState::Failed(error) => Some(error.clone()),
            _ => None,
        }
    }
}

enum ScanOrigin {
    Scope(PathBuf),
    Home,
}

impl fmt::Display for ScanOrigin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scope(scope) => write!(formatter, "{}", scope.display()),
            Self::Home => formatter.write_str("~"),
        }
    }
}

enum ScanState {
    Scanning(Pending<RepoDiscovery>),
    Found(RepoDiscovery),
    Failed(String),
}
