use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    number: u64,
    state: PullRequestState,
    opened_at: u64,
    url: String,
}

impl PullRequest {
    pub fn new(number: u64, state: PullRequestState, opened_at: u64, url: &str) -> Self {
        Self {
            number,
            state,
            opened_at,
            url: url.to_string(),
        }
    }

    pub fn state(&self) -> PullRequestState {
        self.state
    }

    pub fn url(&self) -> &str {
        &self.url
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

impl PullRequestState {
    pub const ALL: [PullRequestState; 4] = [
        PullRequestState::Open,
        PullRequestState::Draft,
        PullRequestState::Merged,
        PullRequestState::Closed,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PullRequestState::Open => "open",
            PullRequestState::Draft => "draft",
            PullRequestState::Merged => "merged",
            PullRequestState::Closed => "closed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct PullRequestFixture;

    impl PullRequestFixture {
        const OPENED_AT: u64 = 1_700_000_000;
        const URL: &'static str = "https://github.example.com/example-org/example-project/pull/7";

        fn open() -> PullRequest {
            PullRequest::new(7, PullRequestState::Open, Self::OPENED_AT, Self::URL)
        }
    }

    #[test]
    fn a_pull_request_shows_a_hash_then_its_number() {
        let pull_request = PullRequest::new(
            142,
            PullRequestState::Open,
            PullRequestFixture::OPENED_AT,
            PullRequestFixture::URL,
        );

        assert_eq!(pull_request.to_string(), "#142");
    }

    #[test]
    fn the_state_is_kept() {
        let pull_request = PullRequest::new(
            7,
            PullRequestState::Merged,
            PullRequestFixture::OPENED_AT,
            PullRequestFixture::URL,
        );

        assert_eq!(pull_request.state(), PullRequestState::Merged);
        assert_eq!(pull_request.to_string(), "#7");
    }

    #[test]
    fn a_pull_request_is_opened_since_its_own_second_and_any_earlier_one() {
        let pull_request = PullRequestFixture::open();

        assert!(pull_request.opened_since(PullRequestFixture::OPENED_AT));
        assert!(pull_request.opened_since(PullRequestFixture::OPENED_AT - 1));
    }

    #[test]
    fn a_pull_request_is_not_opened_since_a_later_second() {
        let pull_request = PullRequestFixture::open();

        assert!(!pull_request.opened_since(PullRequestFixture::OPENED_AT + 1));
    }

    #[test]
    fn a_pull_request_keeps_its_url() {
        assert_eq!(PullRequestFixture::open().url(), PullRequestFixture::URL);
    }

    #[test]
    fn labels_are_stable_lowercase_strings() {
        let labels: Vec<&str> = PullRequestState::ALL
            .iter()
            .map(|state| state.label())
            .collect();

        assert_eq!(labels, ["open", "draft", "merged", "closed"]);
    }
}
