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
            .map(|listing| query.pull_requests(&listing))
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

struct GhQuery {
    branches: Vec<String>,
}

impl GhQuery {
    const NEWEST_FIRST: &'static str = "first: 1, orderBy: {field: CREATED_AT, direction: DESC}";
    const FIELDS: &'static str = "number state isDraft createdAt";
    const RESHAPE: &'static str = ".data.repository \
        | map_values(.nodes[0] | if . then .createdAt |= fromdateiso8601 else . end)";

    fn new(branches: &[String]) -> Self {
        Self {
            branches: branches.to_vec(),
        }
    }

    fn arguments(&self) -> Vec<String> {
        let mut arguments = vec![
            "-f".to_string(),
            format!("query={}", self.text()),
            "--jq".to_string(),
            Self::RESHAPE.to_string(),
        ];
        for (alias, branch) in self.aliased() {
            arguments.extend(["-f".to_string(), format!("{alias}={branch}")]);
        }
        arguments
    }

    fn text(&self) -> String {
        let variables: String = self
            .aliased()
            .map(|(alias, _)| format!(", ${alias}: String!"))
            .collect();
        let fields: String = self
            .aliased()
            .map(|(alias, _)| Self::field(&alias))
            .collect();
        format!(
            "query($owner: String!, $name: String!{variables}) \
             {{ repository(owner: $owner, name: $name) {{{fields} }} }}"
        )
    }

    fn field(alias: &str) -> String {
        format!(
            " {alias}: pullRequests(headRefName: ${alias}, {}) {{ nodes {{ {} }} }}",
            Self::NEWEST_FIRST,
            Self::FIELDS
        )
    }

    fn pull_requests(&self, listing: &GhListing) -> HashMap<String, Option<PullRequest>> {
        self.aliased()
            .map(|(alias, branch)| (branch.clone(), listing.pull_request(&alias)))
            .collect()
    }

    fn aliased(&self) -> impl Iterator<Item = (String, &String)> {
        self.branches
            .iter()
            .enumerate()
            .map(|(index, branch)| (format!("branch{index}"), branch))
    }
}

struct GhListing {
    entries: HashMap<String, Option<GhEntry>>,
}

impl GhListing {
    fn parse(text: &str) -> Result<Self, serde_json::Error> {
        Ok(Self {
            entries: serde_json::from_str(text)?,
        })
    }

    fn pull_request(&self, alias: &str) -> Option<PullRequest> {
        self.entries
            .get(alias)
            .and_then(Option::as_ref)
            .and_then(GhEntry::pull_request)
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
      branch*=fix-auth) entry='{"number":42,"state":"OPEN","isDraft":false,"createdAt":1700000000}' ;;
      branch*=*) entry=null ;;
      *) entry='' ;;
    esac
    if [ -n "$entry" ]; then listing="$listing${listing:+,}\"${2%%=*}\":$entry"; fi
    shift
  fi
  shift
done
printf '{%s}' "$listing"
"#;

        fn pull_request(entry: &str) -> Option<PullRequest> {
            let listing = GhListing::parse(&format!(r#"{{"branch0":{entry}}}"#)).unwrap();
            listing.pull_request("branch0")
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
            r#"{"number":142,"state":"OPEN","isDraft":false,"createdAt":1700000000}"#,
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
            r#"{"number":5,"state":"OPEN","isDraft":true,"createdAt":1700000000}"#,
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
            r#"{"number":9,"state":"MERGED","isDraft":false,"createdAt":1700000000}"#,
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
            r#"{"number":3,"state":"CLOSED","isDraft":false,"createdAt":1700000000}"#,
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
        assert_eq!(ParseFixture::pull_request("null"), None);
    }

    #[test]
    fn an_unknown_state_reads_as_none() {
        assert_eq!(
            ParseFixture::pull_request(
                r#"{"number":1,"state":"QUEUED","isDraft":false,"createdAt":1700000000}"#
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
            r#"{"branch0":{"number":7,"state":"OPEN","isDraft":false,"createdAt":1700000000},"branch1":null}"#,
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
    fn the_query_holds_one_aliased_field_per_branch() {
        let text = GhQuery::new(&ParseFixture::branches()).text();

        assert!(text.contains("$branch0: String!, $branch1: String!"));
        assert!(text.contains("branch1: pullRequests(headRefName: $branch1,"));
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
