use std::collections::HashMap;

use serde::de::Error;
use serde::{Deserialize, Deserializer};

use super::gh_query::GhQuery;
use crate::domain::{PullRequest, PullRequestState};

#[derive(Deserialize)]
pub struct GhListing {
    viewer: GhAccount,
    repository: GhRepository,
}

impl GhListing {
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str::<GhResponse>(text).map(|response| response.data)
    }

    pub fn pull_requests(&self, query: &GhQuery) -> HashMap<String, Option<PullRequest>> {
        query
            .aliased()
            .map(|(alias, branch)| (branch.clone(), self.pull_request(&alias)))
            .collect()
    }

    fn pull_request(&self, alias: &str) -> Option<PullRequest> {
        self.repository
            .candidates(alias)
            .iter()
            .find(|entry| self.owns(entry))
            .and_then(GhEntry::pull_request)
    }

    fn owns(&self, entry: &GhEntry) -> bool {
        entry.has_head_owner(&self.viewer.login) || entry.has_head_owner(self.repository.owner())
    }
}

#[derive(Deserialize)]
struct GhResponse {
    data: GhListing,
}

#[derive(Deserialize)]
struct GhAccount {
    login: String,
}

#[derive(Deserialize)]
struct GhRepository {
    owner: GhAccount,
    #[serde(flatten)]
    branches: HashMap<String, GhConnection>,
}

impl GhRepository {
    fn candidates(&self, alias: &str) -> &[GhEntry] {
        self.branches
            .get(alias)
            .map_or(&[], |connection| &connection.nodes)
    }

    fn owner(&self) -> &str {
        &self.owner.login
    }
}

#[derive(Deserialize)]
struct GhConnection {
    nodes: Vec<GhEntry>,
}

#[derive(Deserialize)]
struct GhEntry {
    number: u64,
    state: String,
    #[serde(default, rename = "isDraft")]
    is_draft: bool,
    #[serde(rename = "createdAt")]
    created_at: GhTimestamp,
    #[serde(rename = "headRepositoryOwner")]
    head_repository_owner: Option<GhAccount>,
}

impl GhEntry {
    fn pull_request(&self) -> Option<PullRequest> {
        let state = match self.state.as_str() {
            "OPEN" if self.is_draft => PullRequestState::Draft,
            "OPEN" => PullRequestState::Open,
            "MERGED" => PullRequestState::Merged,
            "CLOSED" => PullRequestState::Closed,
            _ => return None,
        };
        Some(PullRequest::new(
            self.number,
            state,
            self.created_at.epoch_seconds,
        ))
    }

    fn has_head_owner(&self, login: &str) -> bool {
        self.head_repository_owner
            .as_ref()
            .is_some_and(|owner| owner.login == login)
    }
}

struct GhTimestamp {
    epoch_seconds: u64,
}

impl GhTimestamp {
    fn parse(text: &str) -> Option<Self> {
        let timestamp: jiff::Timestamp = text.parse().ok()?;
        let epoch_seconds = u64::try_from(timestamp.as_second()).ok()?;
        Some(Self { epoch_seconds })
    }
}

impl<'de> Deserialize<'de> for GhTimestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| D::Error::custom(format!("unreadable timestamp {text}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ListingFixture;

    impl ListingFixture {
        const OPENED_AT: u64 = 1_700_000_000;
        const OPENED_AT_TEXT: &'static str = "2023-11-14T22:13:20Z";

        fn pull_request(nodes: &[String]) -> Option<PullRequest> {
            let listing = GhListing::parse(&Self::response(&nodes.join(","))).unwrap();
            listing.pull_request("branch0")
        }

        fn response(nodes: &str) -> String {
            format!(
                r#"{{"data":{{"viewer":{{"login":"octocat"}},"repository":{{"owner":{{"login":"example-org"}},"branch0":{{"nodes":[{nodes}]}}}}}}}}"#
            )
        }

        fn node(number: u64, state: &str, is_draft: bool, head_owner: &str) -> String {
            format!(
                r#"{{"number":{number},"state":"{state}","isDraft":{is_draft},"createdAt":"{}","headRepositoryOwner":{{"login":"{head_owner}"}}}}"#,
                Self::OPENED_AT_TEXT
            )
        }

        fn opened(number: u64, state: PullRequestState) -> Option<PullRequest> {
            Some(PullRequest::new(number, state, Self::OPENED_AT))
        }
    }

    #[test]
    fn an_open_pull_request_is_read_with_its_number_and_opening_time() {
        let found =
            ListingFixture::pull_request(&[ListingFixture::node(142, "OPEN", false, "octocat")]);

        assert_eq!(found, ListingFixture::opened(142, PullRequestState::Open));
    }

    #[test]
    fn a_draft_reads_as_a_draft() {
        let found =
            ListingFixture::pull_request(&[ListingFixture::node(5, "OPEN", true, "octocat")]);

        assert_eq!(found, ListingFixture::opened(5, PullRequestState::Draft));
    }

    #[test]
    fn a_merged_pull_request_reads_as_merged() {
        let found =
            ListingFixture::pull_request(&[ListingFixture::node(9, "MERGED", false, "octocat")]);

        assert_eq!(found, ListingFixture::opened(9, PullRequestState::Merged));
    }

    #[test]
    fn a_closed_pull_request_reads_as_closed() {
        let found =
            ListingFixture::pull_request(&[ListingFixture::node(3, "CLOSED", false, "octocat")]);

        assert_eq!(found, ListingFixture::opened(3, PullRequestState::Closed));
    }

    #[test]
    fn no_pull_request_reads_as_none() {
        assert_eq!(ListingFixture::pull_request(&[]), None);
    }

    #[test]
    fn an_unknown_state_reads_as_none() {
        let found =
            ListingFixture::pull_request(&[ListingFixture::node(1, "QUEUED", false, "octocat")]);

        assert_eq!(found, None);
    }

    #[test]
    fn a_strangers_newer_fork_pull_request_is_skipped() {
        let found = ListingFixture::pull_request(&[
            ListingFixture::node(300, "OPEN", false, "stranger"),
            ListingFixture::node(42, "OPEN", false, "octocat"),
        ]);

        assert_eq!(found, ListingFixture::opened(42, PullRequestState::Open));
    }

    #[test]
    fn a_pull_request_headed_in_the_repository_owner_counts_as_own() {
        let found =
            ListingFixture::pull_request(&[ListingFixture::node(8, "OPEN", false, "example-org")]);

        assert_eq!(found, ListingFixture::opened(8, PullRequestState::Open));
    }

    #[test]
    fn only_strangers_pull_requests_read_as_none() {
        let found =
            ListingFixture::pull_request(&[ListingFixture::node(300, "OPEN", false, "stranger")]);

        assert_eq!(found, None);
    }

    #[test]
    fn unreadable_json_is_a_parse_error() {
        assert!(GhListing::parse("not json").is_err());
    }

    #[test]
    fn an_unreadable_opening_time_is_a_parse_error() {
        let node = ListingFixture::node(1, "OPEN", false, "octocat")
            .replace(ListingFixture::OPENED_AT_TEXT, "yesterday");

        assert!(GhListing::parse(&ListingFixture::response(&node)).is_err());
    }
}
