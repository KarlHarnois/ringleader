use std::collections::HashMap;
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

    pub fn calls(&self) -> Vec<String> {
        self.state.lock().unwrap().calls.clone()
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
        if state.failure {
            return Err(PullRequestError::Failed {
                subcommand: "pr list".to_string(),
                status: "exit status: 1".to_string(),
                stderr: "not logged in".to_string(),
            });
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
