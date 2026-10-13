use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PullRequest {
    number: u64,
    state: PullRequestState,
}

impl PullRequest {
    pub fn new(number: u64, state: PullRequestState) -> Self {
        Self { number, state }
    }

    pub fn state(&self) -> PullRequestState {
        self.state
    }
}

impl fmt::Display for PullRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "#{}", self.number)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestState {
    Open,
    Draft,
    Merged,
    Closed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pull_request_shows_a_hash_then_its_number() {
        let pull_request = PullRequest::new(142, PullRequestState::Open);

        assert_eq!(pull_request.to_string(), "#142");
    }

    #[test]
    fn the_state_is_kept() {
        let pull_request = PullRequest::new(7, PullRequestState::Merged);

        assert_eq!(pull_request.state(), PullRequestState::Merged);
        assert_eq!(pull_request.to_string(), "#7");
    }
}
