use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use super::bounded_command::{BoundedCommand, Call, CallKind, CommandError};
use super::command_output::CommandOutput;
use crate::application::ports::{PullRequestError, PullRequests};
use crate::domain::{PullRequest, PullRequestState};

pub struct GhPullRequests {
    program: OsString,
    bounded: BoundedCommand,
}

impl GhPullRequests {
    const SUBCOMMAND: &'static str = "pr list";

    pub fn new(program: &OsStr, bounded: BoundedCommand) -> Self {
        Self {
            program: program.to_os_string(),
            bounded,
        }
    }

    fn command(program: &OsStr, repo: &Path, branch: &str) -> Command {
        let mut command = Command::new(program);
        command
            .current_dir(repo)
            .env("GH_PROMPT_DISABLED", "1")
            .env("NO_COLOR", "1")
            .args([
                "pr",
                "list",
                "--head",
                branch,
                "--state",
                "all",
                "--limit",
                "1",
                "--json",
                "number,state,isDraft",
            ]);
        command
    }

    fn branch_pull_request(
        &self,
        repo: &Path,
        branch: &str,
    ) -> Result<Option<PullRequest>, PullRequestError> {
        let call = Call::new(Self::SUBCOMMAND, CallKind::GhQuery);
        let output = self
            .bounded
            .run(Self::command(&self.program, repo, branch), &call)
            .map_err(Self::error)?;
        Self::pull_request(&output)
    }

    fn pull_request(output: &CommandOutput) -> Result<Option<PullRequest>, PullRequestError> {
        if !output.succeeded() {
            return Err(PullRequestError::Failed {
                subcommand: Self::SUBCOMMAND.to_string(),
                status: output.status_label(),
                stderr: output.stderr_text(),
            });
        }
        GhListing::parse(&output.stdout_text())
            .map(|listing| listing.first())
            .map_err(|error| PullRequestError::Failed {
                subcommand: Self::SUBCOMMAND.to_string(),
                status: "unreadable json".to_string(),
                stderr: error.to_string(),
            })
    }

    fn error(error: CommandError) -> PullRequestError {
        match error {
            CommandError::TimedOut { seconds, .. } => PullRequestError::TimedOut {
                subcommand: Self::SUBCOMMAND.to_string(),
                seconds,
            },
            CommandError::Spawn { detail, .. } => PullRequestError::Failed {
                subcommand: Self::SUBCOMMAND.to_string(),
                status: "not started".to_string(),
                stderr: detail,
            },
        }
    }
}

impl PullRequests for GhPullRequests {
    fn lookup(
        &self,
        repo: &Path,
        branches: &[String],
    ) -> Result<HashMap<String, Option<PullRequest>>, PullRequestError> {
        branches
            .iter()
            .map(|branch| Ok((branch.clone(), self.branch_pull_request(repo, branch)?)))
            .collect()
    }
}

struct GhListing {
    entries: Vec<GhEntry>,
}

impl GhListing {
    fn parse(text: &str) -> Result<Self, serde_json::Error> {
        Ok(Self {
            entries: serde_json::from_str(text)?,
        })
    }

    fn first(&self) -> Option<PullRequest> {
        self.entries.first().and_then(GhEntry::pull_request)
    }
}

#[derive(Deserialize)]
struct GhEntry {
    number: u64,
    state: String,
    #[serde(default, rename = "isDraft")]
    is_draft: bool,
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
        Some(PullRequest::new(self.number, state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ParseFixture;

    impl ParseFixture {
        fn pull_request(json: &str) -> Option<PullRequest> {
            GhListing::parse(json).unwrap().first()
        }
    }

    #[test]
    fn an_open_pull_request_is_read_with_its_number() {
        let found =
            ParseFixture::pull_request(r#"[{"number":142,"state":"OPEN","isDraft":false}]"#);

        assert_eq!(found, Some(PullRequest::new(142, PullRequestState::Open)));
    }

    #[test]
    fn a_draft_reads_as_a_draft() {
        let found = ParseFixture::pull_request(r#"[{"number":5,"state":"OPEN","isDraft":true}]"#);

        assert_eq!(found, Some(PullRequest::new(5, PullRequestState::Draft)));
    }

    #[test]
    fn a_merged_pull_request_reads_as_merged() {
        let found =
            ParseFixture::pull_request(r#"[{"number":9,"state":"MERGED","isDraft":false}]"#);

        assert_eq!(found, Some(PullRequest::new(9, PullRequestState::Merged)));
    }

    #[test]
    fn a_closed_pull_request_reads_as_closed() {
        let found =
            ParseFixture::pull_request(r#"[{"number":3,"state":"CLOSED","isDraft":false}]"#);

        assert_eq!(found, Some(PullRequest::new(3, PullRequestState::Closed)));
    }

    #[test]
    fn no_pull_request_reads_as_none() {
        assert_eq!(ParseFixture::pull_request("[]"), None);
    }

    #[test]
    fn an_unknown_state_reads_as_none() {
        assert_eq!(
            ParseFixture::pull_request(r#"[{"number":1,"state":"QUEUED","isDraft":false}]"#),
            None
        );
    }

    #[test]
    fn unreadable_json_is_a_parse_error() {
        assert!(GhListing::parse("not json").is_err());
    }
}
