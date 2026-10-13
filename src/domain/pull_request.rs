use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PullRequest {
    number: u64,
    state: PullRequestState,
    opened_at: u64,
}

impl PullRequest {
    pub fn new(number: u64, state: PullRequestState, opened_at: u64) -> Self {
        Self {
            number,
            state,
            opened_at,
        }
    }

    pub fn state(&self) -> PullRequestState {
        self.state
    }

    pub fn opened_since(&self, epoch_seconds: u64) -> bool {
        self.opened_at >= epoch_seconds
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

    struct PullRequestFixture;

    impl PullRequestFixture {
        const OPENED_AT: u64 = 1_700_000_000;
    }

    #[test]
    fn a_pull_request_shows_a_hash_then_its_number() {
        let pull_request =
            PullRequest::new(142, PullRequestState::Open, PullRequestFixture::OPENED_AT);

        assert_eq!(pull_request.to_string(), "#142");
    }

    #[test]
    fn the_state_is_kept() {
        let pull_request =
            PullRequest::new(7, PullRequestState::Merged, PullRequestFixture::OPENED_AT);

        assert_eq!(pull_request.state(), PullRequestState::Merged);
        assert_eq!(pull_request.to_string(), "#7");
    }

    #[test]
    fn a_pull_request_is_opened_since_its_own_second_and_any_earlier_one() {
        let pull_request =
            PullRequest::new(7, PullRequestState::Open, PullRequestFixture::OPENED_AT);

        assert!(pull_request.opened_since(PullRequestFixture::OPENED_AT));
        assert!(pull_request.opened_since(PullRequestFixture::OPENED_AT - 1));
    }

    #[test]
    fn a_pull_request_is_not_opened_since_a_later_second() {
        let pull_request =
            PullRequest::new(7, PullRequestState::Open, PullRequestFixture::OPENED_AT);

        assert!(!pull_request.opened_since(PullRequestFixture::OPENED_AT + 1));
    }
}
