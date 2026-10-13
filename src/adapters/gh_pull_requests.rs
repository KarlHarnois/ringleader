use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Command;

use super::bounded_command::{BoundedCommand, Call, CallKind, CommandError};
use super::command_output::CommandOutput;
use super::gh_listing::GhListing;
use super::gh_query::GhQuery;
use super::git_invocation::GitInvocation;
use crate::application::ports::{PullRequestError, PullRequests};
use crate::domain::PullRequest;

pub struct GhPullRequests {
    program: OsString,
    bounded: BoundedCommand,
}

impl GhPullRequests {
    const SUBCOMMAND: &'static str = "api graphql";
    const REPOSITORY_OVERRIDE: &'static str = "GH_REPO";

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
            .env_remove(Self::REPOSITORY_OVERRIDE)
            .env("GH_PROMPT_DISABLED", "1")
            .env("NO_COLOR", "1")
            .args(["api", "graphql", "-F", "owner={owner}", "-F", "name={repo}"])
            .args(query.arguments());
        for name in GitInvocation::REPOSITORY_LOCAL_VARIABLES {
            command.env_remove(name);
        }
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::ExitStatusExt;
    use std::path::PathBuf;
    use std::process::ExitStatus;

    use super::*;
    use crate::adapters::bounded_command::Deadlines;
    use crate::domain::PullRequestState;

    struct ParseFixture;

    impl ParseFixture {
        const OPENED_AT: u64 = 1_700_000_000;
        const RECORDED: &'static str =
            include_str!("../../tests/fixtures/gh-graphql-pull-requests.json");
        const STAND_IN: &'static str = r#"#!/bin/sh
echo call >> "$0.calls"
listing=''
while [ $# -gt 0 ]; do
  if [ "$1" = "-f" ]; then
    case "$2" in
      branch*=fix-auth) entry='{"nodes":[{"number":42,"state":"OPEN","isDraft":false,"createdAt":"2023-11-14T22:13:20Z","headRepositoryOwner":{"login":"octocat"}}]}' ;;
      branch*=*) entry='{"nodes":[]}' ;;
      *) entry='' ;;
    esac
    if [ -n "$entry" ]; then listing="$listing${listing:+,}\"${2%%=*}\":$entry"; fi
    shift
  fi
  shift
done
printf '{"data":{"viewer":{"login":"octocat"},"repository":{"owner":{"login":"example-org"},%s}}}' "$listing"
"#;

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
    fn a_recorded_response_maps_each_alias_back_to_its_branch() {
        let query = GhQuery::new(&ParseFixture::branches());
        let output = ParseFixture::output(0, ParseFixture::RECORDED);

        let found = GhPullRequests::pull_requests(&output, &query).unwrap();

        assert_eq!(
            found.get("fix-auth"),
            Some(&Some(PullRequest::new(
                42,
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
    fn the_command_ignores_inherited_repository_overrides() {
        let query = GhQuery::new(&ParseFixture::branches());
        let command =
            GhPullRequests::command(OsStr::new("gh"), Path::new("/repos/example"), &query);
        let removed: Vec<&OsStr> = command
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name)
            .collect();

        assert!(removed.contains(&OsStr::new("GH_REPO")));
        assert!(removed.contains(&OsStr::new("GIT_DIR")));
        assert!(removed.contains(&OsStr::new("GIT_WORK_TREE")));
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
