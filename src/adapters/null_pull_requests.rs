use std::collections::HashMap;
use std::path::Path;

use crate::application::ports::{PullRequestError, PullRequests};
use crate::domain::PullRequest;

pub struct NullPullRequests;

impl PullRequests for NullPullRequests {
    fn lookup(
        &self,
        _repo: &Path,
        branches: &[String],
    ) -> HashMap<String, Result<Option<PullRequest>, PullRequestError>> {
        branches
            .iter()
            .map(|branch| (branch.clone(), Ok(None)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_branch_has_no_pull_request() {
        let branches = vec!["main".to_string(), "fix-auth".to_string()];

        let found = NullPullRequests.lookup(Path::new("/repos/example-project"), &branches);

        assert_eq!(found.len(), branches.len());
        assert!(found.values().all(|result| matches!(result, Ok(None))));
    }
}
