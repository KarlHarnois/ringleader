use crate::application::{Pending, RepoDiscovery, SessionService};
use crate::domain::Repo;

pub struct RepoScan {
    state: ScanState,
}

impl RepoScan {
    pub fn start(service: &SessionService) -> Self {
        Self {
            state: ScanState::Scanning(service.find_repos()),
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

    pub fn repos(&self) -> &[Repo] {
        match &self.state {
            ScanState::Found(found) => found.repos(),
            _ => &[],
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

enum ScanState {
    Scanning(Pending<RepoDiscovery>),
    Found(RepoDiscovery),
    Failed(String),
}
