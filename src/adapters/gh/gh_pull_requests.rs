use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard, PoisonError};

use super::gh_listing::GhListing;
use super::gh_query::GhQuery;
use crate::adapters::BoundedCommand;
use crate::adapters::child_process::bounded_command::{Call, CommandError};
use crate::adapters::child_process::command_output::CommandOutput;
use crate::adapters::child_process::deadlines::CallKind;
use crate::adapters::git::git_invocation::GitInvocation;
use crate::application::ports::{PullRequestError, PullRequests};
use crate::domain::PullRequest;

pub struct GhPullRequests {
    program: OsString,
    bounded: BoundedCommand,
    hosts: Mutex<HashMap<PathBuf, String>>,
}

impl GhPullRequests {
    const SUBCOMMAND: &'static str = "api graphql";
    const BROWSE: &'static str = "browse";
    const REPOSITORY_OVERRIDE: &'static str = "GH_REPO";

    pub fn new(program: &OsStr, bounded: BoundedCommand) -> Self {
        Self {
            program: program.to_os_string(),
            bounded,
            hosts: Mutex::default(),
        }
    }

    fn host(&self, repo: &Path) -> Result<String, PullRequestError> {
        if let Some(host) = self.known_hosts().get(repo) {
            return Ok(host.clone());
        }
        let host = self.resolve_host(repo)?;
        self.known_hosts().insert(repo.to_path_buf(), host.clone());
        Ok(host)
    }

    fn resolve_host(&self, repo: &Path) -> Result<String, PullRequestError> {
        let call = Call::new(Self::BROWSE, CallKind::GhQuery);
        let output = self
            .bounded
            .run(self.browse_command(repo), &call)
            .map_err(|error| Self::error(Self::BROWSE, error))?;
        if !output.succeeded() {
            return Err(Self::failure(Self::BROWSE, &output));
        }
        let url = output.stdout_text();
        Self::url_host(&url).ok_or_else(|| PullRequestError::Failed {
            subcommand: Self::BROWSE.to_string(),
            status: "unreadable url".to_string(),
            stderr: url.clone(),
        })
    }

    fn known_hosts(&self) -> MutexGuard<'_, HashMap<PathBuf, String>> {
        self.hosts.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn browse_command(&self, repo: &Path) -> Command {
        let mut command = self.gh(repo);
        command.args(["browse", "--no-browser"]);
        command
    }

    fn graphql_command(&self, repo: &Path, host: &str, query: &GhQuery) -> Command {
        let mut command = self.gh(repo);
        command
            .args(["api", "graphql", "--hostname", host])
            .args(["-F", "owner={owner}", "-F", "name={repo}"])
            .args(query.arguments());
        command
    }

    fn gh(&self, repo: &Path) -> Command {
        let mut command = Command::new(&self.program);
        command
            .current_dir(repo)
            .env_remove(Self::REPOSITORY_OVERRIDE)
            .env("GH_PROMPT_DISABLED", "1")
            .env("NO_COLOR", "1");
        for name in GitInvocation::REPOSITORY_LOCAL_VARIABLES {
            command.env_remove(name);
        }
        command
    }

    fn url_host(url: &str) -> Option<String> {
        let (_, address) = url.trim().split_once("://")?;
        let host = address.split('/').next()?;
        (!host.is_empty()).then(|| host.to_string())
    }

    fn pull_requests(
        output: &CommandOutput,
        query: &GhQuery,
    ) -> Result<HashMap<String, Option<PullRequest>>, PullRequestError> {
        if !output.succeeded() {
            return Err(Self::failure(Self::SUBCOMMAND, output));
        }
        GhListing::parse(&output.stdout_text())
            .map(|listing| listing.pull_requests(query))
            .map_err(|error| PullRequestError::Failed {
                subcommand: Self::SUBCOMMAND.to_string(),
                status: "unreadable json".to_string(),
                stderr: error.to_string(),
            })
    }

    fn failure(subcommand: &str, output: &CommandOutput) -> PullRequestError {
        PullRequestError::Failed {
            subcommand: subcommand.to_string(),
            status: output.status_label(),
            stderr: output.stderr_text(),
        }
    }

    fn error(subcommand: &str, error: CommandError) -> PullRequestError {
        match error {
            CommandError::TimedOut { seconds, .. } => PullRequestError::TimedOut {
                subcommand: subcommand.to_string(),
                seconds,
            },
            CommandError::Spawn { detail, .. } => PullRequestError::Failed {
                subcommand: subcommand.to_string(),
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
        let host = self.host(repo)?;
        let query = GhQuery::new(branches);
        let call = Call::new(Self::SUBCOMMAND, CallKind::GhQuery);
        let output = self
            .bounded
            .run(self.graphql_command(repo, &host, &query), &call)
            .map_err(|error| Self::error(Self::SUBCOMMAND, error))?;
        Self::pull_requests(&output, &query)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::process::ExitStatusExt;
    use std::path::PathBuf;
    use std::process::ExitStatus;

    use super::*;
    use crate::adapters::Deadlines;
    use crate::domain::PullRequestState;
    use crate::test_support::{ExecutableScript, ServiceFixture};

    struct ParseFixture;

    impl ParseFixture {
        const OPENED_AT: u64 = 1_700_000_000;
        const RECORDED: &'static str =
            include_str!("../../../tests/fixtures/gh-graphql-pull-requests.json");
        const HOST: &'static str = "github.example.com";
        const STAND_IN: &'static str = r#"#!/bin/sh
echo "$*" >> "$0.calls"
if [ "$1" = "browse" ]; then
  printf 'https://github.example.com/example-org/example-project\n'
  exit 0
fi
listing=''
while [ $# -gt 0 ]; do
  if [ "$1" = "-f" ]; then
    case "$2" in
      branch*=fix-auth) entry='{"nodes":[{"number":42,"url":"https://github.example.com/example-org/example-project/pull/42","state":"OPEN","isDraft":false,"createdAt":"2023-11-14T22:13:20Z","headRepositoryOwner":{"login":"octocat"}}]}' ;;
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

        fn fix_auth_pull_request() -> PullRequest {
            let url = ServiceFixture::pull_request_url(42);
            PullRequest::new(42, PullRequestState::Open, Self::OPENED_AT, &url)
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
            ExecutableScript::write(&path, body);
            path
        }

        fn calls(directory: &tempfile::TempDir, subcommand: &str) -> Vec<String> {
            fs::read_to_string(directory.path().join("fake-gh.calls"))
                .unwrap_or_default()
                .lines()
                .filter(|line| line.starts_with(subcommand))
                .map(str::to_string)
                .collect()
        }

        fn named_gh() -> GhPullRequests {
            GhPullRequests::new(OsStr::new("gh"), BoundedCommand::new(Deadlines::standard()))
        }
    }

    #[test]
    fn a_recorded_response_maps_each_alias_back_to_its_branch() {
        let query = GhQuery::new(&ParseFixture::branches());
        let output = ParseFixture::output(0, ParseFixture::RECORDED);

        let found = GhPullRequests::pull_requests(&output, &query).unwrap();

        assert_eq!(
            found.get("fix-auth"),
            Some(&Some(ParseFixture::fix_auth_pull_request()))
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
        let mapped = GhPullRequests::error(
            GhPullRequests::SUBCOMMAND,
            CommandError::TimedOut {
                subcommand: "gh api graphql".to_string(),
                seconds: 20,
            },
        );

        assert!(matches!(
            mapped,
            PullRequestError::TimedOut { seconds: 20, .. }
        ));
    }

    #[test]
    fn a_spawn_failure_maps_to_a_failure() {
        let mapped = GhPullRequests::error(
            GhPullRequests::SUBCOMMAND,
            CommandError::Spawn {
                subcommand: "gh api graphql".to_string(),
                detail: "not found".to_string(),
            },
        );

        assert!(matches!(mapped, PullRequestError::Failed { .. }));
    }

    #[test]
    fn the_command_asks_the_repos_host_for_every_branch_at_once() {
        let query = GhQuery::new(&ParseFixture::branches());
        let command = ParseFixture::named_gh().graphql_command(
            Path::new("/repos/example"),
            ParseFixture::HOST,
            &query,
        );
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let prompt = command
            .get_envs()
            .find(|(name, _)| *name == "GH_PROMPT_DISABLED")
            .and_then(|(_, value)| value);

        assert_eq!(
            args[..8],
            [
                "api",
                "graphql",
                "--hostname",
                ParseFixture::HOST,
                "-F",
                "owner={owner}",
                "-F",
                "name={repo}"
            ]
        );
        assert_eq!(
            args[args.len() - 4..],
            ["-f", "branch0=fix-auth", "-f", "branch1=dark-mode"]
        );
        assert_eq!(command.get_current_dir(), Some(Path::new("/repos/example")));
        assert_eq!(prompt, Some(OsStr::new("1")));
    }

    #[test]
    fn every_gh_call_ignores_inherited_repository_overrides() {
        let command = ParseFixture::named_gh().gh(Path::new("/repos/example"));
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
            Some(&Some(ParseFixture::fix_auth_pull_request()))
        );
        assert_eq!(found.get("dark-mode"), Some(&None));
        assert_eq!(ParseFixture::calls(&directory, "api").len(), 1);
    }

    #[test]
    fn lookup_sends_the_query_to_the_host_of_the_repos_remote() {
        let directory = tempfile::tempdir().unwrap();
        let adapter = ParseFixture::adapter(&directory, ParseFixture::STAND_IN);

        adapter
            .lookup(directory.path(), &ParseFixture::branches())
            .unwrap();

        let queries = ParseFixture::calls(&directory, "api");
        assert!(queries[0].contains("--hostname github.example.com"));
    }

    #[test]
    fn a_repos_host_is_resolved_once() {
        let directory = tempfile::tempdir().unwrap();
        let adapter = ParseFixture::adapter(&directory, ParseFixture::STAND_IN);

        for _ in 0..2 {
            adapter
                .lookup(directory.path(), &ParseFixture::branches())
                .unwrap();
        }

        assert_eq!(ParseFixture::calls(&directory, "browse").len(), 1);
        assert_eq!(ParseFixture::calls(&directory, "api").len(), 2);
    }

    #[test]
    fn a_failed_host_lookup_is_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let adapter = ParseFixture::adapter(
            &directory,
            "#!/bin/sh\n[ \"$1\" = browse ] && exit 1\nexit 0\n",
        );

        let found = adapter.lookup(directory.path(), &ParseFixture::branches());

        assert!(matches!(
            found,
            Err(PullRequestError::Failed { subcommand, .. }) if subcommand == "browse"
        ));
    }

    #[test]
    fn a_url_names_its_host() {
        assert_eq!(
            GhPullRequests::url_host("https://github.example.com/example-org/example-project\n"),
            Some(ParseFixture::HOST.to_string())
        );
    }

    #[test]
    fn text_without_a_scheme_names_no_host() {
        assert_eq!(GhPullRequests::url_host("not a url"), None);
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
        assert!(ParseFixture::calls(&directory, "").is_empty());
    }
}
