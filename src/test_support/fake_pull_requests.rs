use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::application::ports::{PullRequestError, PullRequests};
use crate::domain::PullRequest;

#[derive(Default)]
pub struct FakePullRequests {
    state: Mutex<FakeState>,
}

#[derive(Default)]
struct FakeState {
    pull_requests: HashMap<(PathBuf, String), PullRequest>,
    failing_repos: HashSet<PathBuf>,
    calls: Vec<String>,
    failure: bool,
}

impl FakePullRequests {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_pull_request(self, repo: &Path, branch: &str, pull_request: PullRequest) -> Self {
        self.state
            .lock()
            .unwrap()
            .pull_requests
            .insert((repo.to_path_buf(), branch.to_string()), pull_request);
        self
    }

    pub fn fail(&self) {
        self.state.lock().unwrap().failure = true;
    }

    pub fn recover(&self) {
        self.state.lock().unwrap().failure = false;
    }

    pub fn failing_repo(self, repo: &Path) -> Self {
        self.state
            .lock()
            .unwrap()
            .failing_repos
            .insert(repo.to_path_buf());
        self
    }

    pub fn calls(&self) -> Vec<String> {
        self.state.lock().unwrap().calls.clone()
    }

    fn failure() -> PullRequestError {
        PullRequestError::Failed {
            subcommand: "api graphql".to_string(),
            status: "exit status: 1".to_string(),
            stderr: "not logged in".to_string(),
        }
    }
}

impl PullRequests for FakePullRequests {
    fn lookup(
        &self,
        repo: &Path,
        branches: &[String],
    ) -> Result<HashMap<String, Option<PullRequest>>, PullRequestError> {
        let mut state = self.state.lock().unwrap();
        state
            .calls
            .push(format!("lookup {} {}", repo.display(), branches.len()));
        if state.failure || state.failing_repos.contains(repo) {
            return Err(Self::failure());
        }
        let repo = repo.to_path_buf();
        Ok(branches
            .iter()
            .map(|branch| {
                let found = state
                    .pull_requests
                    .get(&(repo.clone(), branch.clone()))
                    .copied();
                (branch.clone(), found)
            })
            .collect())
    }
}
