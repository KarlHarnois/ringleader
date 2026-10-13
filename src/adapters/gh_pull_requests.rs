use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use super::bounded_command::{BoundedCommand, Call, CallKind, CommandError};
use super::command_output::CommandOutput;
use super::gh_query::GhQuery;
use crate::application::ports::{PullRequestError, PullRequests};
use crate::domain::{PullRequest, PullRequestState};

pub struct GhPullRequests {
    program: OsString,
    bounded: BoundedCommand,
}

impl GhPullRequests {
    const SUBCOMMAND: &'static str = "api graphql";

    pub fn new(program: &OsStr, bounded: BoundedCommand) -> Self {
        Self {
            program: program.to_os_string(),
            bounded,
        }
    }

    fn command(program: &OsStr, repo: &Path, query: &GhQuery) -> Command {
        let mut command = Command::new(program);
        command
            .current_dir(repo)
            .env("GH_PROMPT_DISABLED", "1")
            .env("NO_COLOR", "1")
            .args(["api", "graphql", "-F", "owner={owner}", "-F", "name={repo}"])
            .args(query.arguments());
        command
    }

    fn pull_requests(
        output: &CommandOutput,
        query: &GhQuery,
    ) -> Result<HashMap<String, Option<PullRequest>>, PullRequestError> {
        if !output.succeeded() {
            return Err(PullRequestError::Failed {
                subcommand: Self::SUBCOMMAND.to_string(),
                status: output.status_label(),
                stderr: output.stderr_text(),
            });
        }
        GhListing::parse(&output.stdout_text())
            .map(|listing| listing.pull_requests(query))
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
        if branches.is_empty() {
            return Ok(HashMap::new());
        }
        let query = GhQuery::new(branches);
        let call = Call::new(Self::SUBCOMMAND, CallKind::GhQuery);
        let output = self
            .bounded
            .run(Self::command(&self.program, repo, &query), &call)
            .map_err(Self::error)?;
        Self::pull_requests(&output, &query)
    }
}

#[derive(Deserialize)]
struct GhListing {
    viewer: String,
    owner: String,
    branches: HashMap<String, Vec<GhEntry>>,
}

impl GhListing {
    fn parse(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    fn pull_requests(&self, query: &GhQuery) -> HashMap<String, Option<PullRequest>> {
        query
            .aliased()
            .map(|(alias, branch)| (branch.clone(), self.pull_request(&alias)))
            .collect()
    }

    fn pull_request(&self, alias: &str) -> Option<PullRequest> {
        self.branches
            .get(alias)?
            .iter()
            .find(|entry| self.owns(entry))
            .and_then(GhEntry::pull_request)
    }

    fn owns(&self, entry: &GhEntry) -> bool {
        entry.has_head_owner(&self.viewer) || entry.has_head_owner(&self.owner)
    }
}

#[derive(Deserialize)]
struct GhEntry {
    number: u64,
    state: String,
    #[serde(default, rename = "isDraft")]
    is_draft: bool,
    #[serde(rename = "createdAt")]
    created_at: u64,
    #[serde(rename = "headOwner")]
    head_owner: Option<String>,
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
        Some(PullRequest::new(self.number, state, self.created_at))
    }

    fn has_head_owner(&self, login: &str) -> bool {
        self.head_owner.as_deref() == Some(login)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::ExitStatusExt;
    use std::path::PathBuf;
    use std::process::ExitStatus;

    use super::*;
    use crate::adapters::bounded_command::Deadlines;

    struct ParseFixture;

    impl ParseFixture {
        const OPENED_AT: u64 = 1_700_000_000;
        const STAND_IN: &'static str = r#"#!/bin/sh
echo call >> "$0.calls"
listing=''
while [ $# -gt 0 ]; do
  if [ "$1" = "-f" ]; then
    case "$2" in
      branch*=fix-auth) entry='[{"number":42,"state":"OPEN","isDraft":false,"createdAt":1700000000,"headOwner":"octocat"}]' ;;
      branch*=*) entry='[]' ;;
      *) entry='' ;;
    esac
    if [ -n "$entry" ]; then listing="$listing${listing:+,}\"${2%%=*}\":$entry"; fi
    shift
  fi
  shift
done
printf '{"viewer":"octocat","owner":"example-org","branches":{%s}}' "$listing"
"#;

        fn pull_request(entries: &str) -> Option<PullRequest> {
            let listing = GhListing::parse(&Self::listing(&format!(r#""branch0":[{entries}]"#)));
            listing.unwrap().pull_request("branch0")
        }

        fn listing(branches: &str) -> String {
            format!(r#"{{"viewer":"octocat","owner":"example-org","branches":{{{branches}}}}}"#)
        }

        fn branches() -> Vec<String> {
            vec!["fix-auth".to_string(), "dark-mode".to_string()]
        }

        fn output(code: i32, stdout: &str) -> CommandOutput {
            CommandOutput::new(
                ExitStatus::from_raw(code << 8),
                stdout.as_bytes().to_vec(),
                Vec::new(),
            )
        }

        fn adapter(directory: &tempfile::TempDir, body: &str) -> GhPullRequests {
            let path = Self::executable(directory, body);
            GhPullRequests::new(path.as_os_str(), BoundedCommand::new(Deadlines::standard()))
        }

        fn executable(directory: &tempfile::TempDir, body: &str) -> PathBuf {
            let path = directory.path().join("fake-gh");
            fs::write(&path, body).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path
        }

        fn calls(directory: &tempfile::TempDir) -> usize {
            fs::read_to_string(directory.path().join("fake-gh.calls"))
                .map_or(0, |text| text.lines().count())
        }
    }

    #[test]
    fn an_open_pull_request_is_read_with_its_number() {
        let found = ParseFixture::pull_request(
            r#"{"number":142,"state":"OPEN","isDraft":false,"createdAt":1700000000,"headOwner":"octocat"}"#,
        );

        assert_eq!(
            found,
            Some(PullRequest::new(
                142,
                PullRequestState::Open,
                ParseFixture::OPENED_AT
            ))
        );
    }

    #[test]
    fn a_draft_reads_as_a_draft() {
        let found = ParseFixture::pull_request(
            r#"{"number":5,"state":"OPEN","isDraft":true,"createdAt":1700000000,"headOwner":"octocat"}"#,
        );

        assert_eq!(
            found,
            Some(PullRequest::new(
                5,
                PullRequestState::Draft,
                ParseFixture::OPENED_AT
            ))
        );
    }

    #[test]
    fn a_merged_pull_request_reads_as_merged() {
        let found = ParseFixture::pull_request(
            r#"{"number":9,"state":"MERGED","isDraft":false,"createdAt":1700000000,"headOwner":"octocat"}"#,
        );

        assert_eq!(
            found,
            Some(PullRequest::new(
                9,
                PullRequestState::Merged,
                ParseFixture::OPENED_AT
            ))
        );
    }

    #[test]
    fn a_closed_pull_request_reads_as_closed() {
        let found = ParseFixture::pull_request(
            r#"{"number":3,"state":"CLOSED","isDraft":false,"createdAt":1700000000,"headOwner":"octocat"}"#,
        );

        assert_eq!(
            found,
            Some(PullRequest::new(
                3,
                PullRequestState::Closed,
                ParseFixture::OPENED_AT
            ))
        );
    }

    #[test]
    fn no_pull_request_reads_as_none() {
        assert_eq!(ParseFixture::pull_request(""), None);
    }

    #[test]
    fn a_strangers_newer_fork_pull_request_is_skipped() {
        let found = ParseFixture::pull_request(
            r#"{"number":300,"state":"OPEN","isDraft":false,"createdAt":1700000900,"headOwner":"stranger"},
               {"number":42,"state":"OPEN","isDraft":false,"createdAt":1700000000,"headOwner":"octocat"}"#,
        );

        assert_eq!(
            found,
            Some(PullRequest::new(
                42,
                PullRequestState::Open,
                ParseFixture::OPENED_AT
            ))
        );
    }

    #[test]
    fn a_pull_request_headed_in_the_repository_owner_counts_as_own() {
        let found = ParseFixture::pull_request(
            r#"{"number":8,"state":"OPEN","isDraft":false,"createdAt":1700000000,"headOwner":"example-org"}"#,
        );

        assert_eq!(
            found,
            Some(PullRequest::new(
                8,
                PullRequestState::Open,
                ParseFixture::OPENED_AT
            ))
        );
    }

    #[test]
    fn only_strangers_pull_requests_read_as_none() {
        let found = ParseFixture::pull_request(
            r#"{"number":300,"state":"OPEN","isDraft":false,"createdAt":1700000900,"headOwner":"stranger"}"#,
        );

        assert_eq!(found, None);
    }

    #[test]
    fn an_unknown_state_reads_as_none() {
        assert_eq!(
            ParseFixture::pull_request(
                r#"{"number":1,"state":"QUEUED","isDraft":false,"createdAt":1700000000,"headOwner":"octocat"}"#
            ),
            None
        );
    }

    #[test]
    fn unreadable_json_is_a_parse_error() {
        assert!(GhListing::parse("not json").is_err());
    }

    #[test]
    fn a_successful_listing_maps_each_alias_back_to_its_branch() {
        let query = GhQuery::new(&ParseFixture::branches());
        let output = ParseFixture::output(
            0,
            &ParseFixture::listing(
                r#""branch0":[{"number":7,"state":"OPEN","isDraft":false,"createdAt":1700000000,"headOwner":"octocat"}],"branch1":[]"#,
            ),
        );

        let found = GhPullRequests::pull_requests(&output, &query).unwrap();

        assert_eq!(
            found.get("fix-auth"),
            Some(&Some(PullRequest::new(
                7,
                PullRequestState::Open,
                ParseFixture::OPENED_AT
            )))
        );
        assert_eq!(found.get("dark-mode"), Some(&None));
    }

    #[test]
    fn a_failed_call_is_an_error_not_a_missing_pull_request() {
        let query = GhQuery::new(&ParseFixture::branches());
        let output = ParseFixture::output(1, "");

        assert!(matches!(
            GhPullRequests::pull_requests(&output, &query),
            Err(PullRequestError::Failed { .. })
        ));
    }

    #[test]
    fn a_timeout_maps_to_a_timeout_error() {
        let mapped = GhPullRequests::error(CommandError::TimedOut {
            subcommand: "gh api graphql".to_string(),
            seconds: 20,
        });

        assert!(matches!(
            mapped,
            PullRequestError::TimedOut { seconds: 20, .. }
        ));
    }

    #[test]
    fn a_spawn_failure_maps_to_a_failure() {
        let mapped = GhPullRequests::error(CommandError::Spawn {
            subcommand: "gh api graphql".to_string(),
            detail: "not found".to_string(),
        });

        assert!(matches!(mapped, PullRequestError::Failed { .. }));
    }

    #[test]
    fn the_command_asks_gh_for_every_branch_of_the_repo_at_once() {
        let query = GhQuery::new(&ParseFixture::branches());
        let command =
            GhPullRequests::command(OsStr::new("gh"), Path::new("/repos/example"), &query);
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let prompt = command
            .get_envs()
            .find(|(name, _)| *name == "GH_PROMPT_DISABLED")
            .and_then(|(_, value)| value);

        assert_eq!(
            args[..6],
            ["api", "graphql", "-F", "owner={owner}", "-F", "name={repo}"]
        );
        assert_eq!(
            args[args.len() - 4..],
            ["-f", "branch0=fix-auth", "-f", "branch1=dark-mode"]
        );
        assert_eq!(command.get_current_dir(), Some(Path::new("/repos/example")));
        assert_eq!(prompt, Some(OsStr::new("1")));
    }

    #[test]
    fn lookup_reads_every_branch_in_one_gh_call() {
        let directory = tempfile::tempdir().unwrap();
        let adapter = ParseFixture::adapter(&directory, ParseFixture::STAND_IN);

        let found = adapter
            .lookup(directory.path(), &ParseFixture::branches())
            .unwrap();

        assert_eq!(
            found.get("fix-auth"),
            Some(&Some(PullRequest::new(
                42,
                PullRequestState::Open,
                ParseFixture::OPENED_AT
            )))
        );
        assert_eq!(found.get("dark-mode"), Some(&None));
        assert_eq!(ParseFixture::calls(&directory), 1);
    }

    #[test]
    fn lookup_reports_a_failed_call_as_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let adapter = ParseFixture::adapter(&directory, "#!/bin/sh\nexit 1\n");

        let found = adapter.lookup(directory.path(), &ParseFixture::branches());

        assert!(matches!(found, Err(PullRequestError::Failed { .. })));
    }

    #[test]
    fn lookup_of_no_branches_runs_no_gh() {
        let directory = tempfile::tempdir().unwrap();
        let adapter = ParseFixture::adapter(&directory, ParseFixture::STAND_IN);

        let found = adapter.lookup(directory.path(), &[]).unwrap();

        assert!(found.is_empty());
        assert_eq!(ParseFixture::calls(&directory), 0);
    }
}
