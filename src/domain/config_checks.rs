use std::path::Path;

use super::agent::Agent;
use super::config_draft::ConfigDraft;
use super::repo_name::RepoName;

pub struct ConfigChecks<'a> {
    draft: &'a ConfigDraft,
    failures: Vec<String>,
}

impl<'a> ConfigChecks<'a> {
    const WIDTHS: (u16, u16) = (20, 500);
    const HEIGHTS: (u16, u16) = (10, 200);
    const IDLE_SECONDS: (i64, i64) = (2, 3600);
    const LAUNCHER_VARIABLES: [&'static str; 5] =
        ["TERM", "COLORTERM", "TMUX", "TMUX_PANE", "GPG_TTY"];
    const TMUX_UPDATED_VARIABLES: [&'static str; 5] = [
        "MSYSTEM",
        "SSH_ASKPASS",
        "SSH_AGENT_PID",
        "SSH_CONNECTION",
        "WINDOWID",
    ];

    pub fn new(draft: &'a ConfigDraft) -> Self {
        Self {
            draft,
            failures: Vec::new(),
        }
    }

    pub fn failures(mut self) -> Vec<String> {
        self.check_prefix("tmux_prefix", self.draft.raw_tmux_prefix(), '-');
        self.check_prefix("branch_prefix", self.draft.raw_branch_prefix(), '/');
        self.check_absolute("worktree_root", self.draft.raw_worktree_root());
        self.check_session_size();
        self.check_idle_seconds();
        self.check_scan_depth();
        self.check_repos();
        self.check_agents();
        self.failures
    }

    fn fail(&mut self, failure: String) {
        self.failures.push(failure);
    }

    fn check_prefix(&mut self, key: &str, prefix: &str, ending: char) {
        let body = prefix.strip_suffix(ending).unwrap_or("");
        let starts_well = body
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit());
        let body_well_formed = body.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        });
        if !(starts_well && body_well_formed) {
            self.fail(format!(
                "{key}: `{prefix}` must start with a lowercase letter or digit, use only a-z, 0-9, and -, and end with `{ending}`"
            ));
        }
    }

    fn check_absolute(&mut self, key: &str, path: &Path) {
        let text = path.to_string_lossy();
        if text.starts_with('$') {
            self.fail(format!(
                "{key}: `{text}` starts with $, environment variables are not expanded, use an absolute or ~/ path"
            ));
        } else if !path.is_absolute() {
            self.fail(format!("{key}: `{text}` is not an absolute path"));
        }
    }

    fn check_session_size(&mut self) {
        let in_range = self.draft.parsed_session_size().is_some_and(|size| {
            (Self::WIDTHS.0..=Self::WIDTHS.1).contains(&size.width())
                && (Self::HEIGHTS.0..=Self::HEIGHTS.1).contains(&size.height())
        });
        if !in_range {
            self.fail(format!(
                "session_size: `{}` must be <width>x<height> with a width from {} to {} and a height from {} to {}",
                self.draft.raw_session_size(),
                Self::WIDTHS.0,
                Self::WIDTHS.1,
                Self::HEIGHTS.0,
                Self::HEIGHTS.1
            ));
        }
    }

    fn check_idle_seconds(&mut self) {
        let seconds = self.draft.raw_activity_idle_seconds();
        if !(Self::IDLE_SECONDS.0..=Self::IDLE_SECONDS.1).contains(&seconds) {
            self.fail(format!(
                "activity_idle_seconds: {seconds} must be from {} to {}",
                Self::IDLE_SECONDS.0,
                Self::IDLE_SECONDS.1
            ));
        }
    }

    fn check_scan_depth(&mut self) {
        let depth = self.draft.raw_scan_depth();
        if depth < 0 {
            self.fail(format!("scan_depth: {depth} must not be negative"));
        }
    }

    fn check_repos(&mut self) {
        let mut seen: Vec<RepoName> = Vec::new();
        for repo in self.draft.repo_drafts() {
            self.check_absolute("repos.path", repo.path());
            match repo.resolved_name() {
                None => self.fail(format!(
                    "repos.name: `{}` is not a valid repo name",
                    repo.shown_name()
                )),
                Some(name) if seen.contains(&name) => {
                    self.fail(format!("repos.name: `{name}` names two [[repos]] entries"))
                }
                Some(name) => seen.push(name),
            }
        }
    }

    fn check_agents(&mut self) {
        let mut seen: Vec<&str> = Vec::new();
        for agent in self.draft.agents_value() {
            if seen.contains(&agent.name()) {
                self.fail(format!(
                    "agents.name: `{}` names two [[agents]] entries",
                    agent.name()
                ));
            }
            seen.push(agent.name());
            self.check_command(agent);
            for name in agent.env_names() {
                self.check_env_name(agent, name);
            }
        }
    }

    fn check_command(&mut self, agent: &Agent) {
        let command = agent.command();
        if command.is_empty() {
            self.fail(format!(
                "agents.command: agent `{}` has an empty command",
                agent.name()
            ));
        } else if command.contains('/') && !Path::new(command).is_absolute() {
            self.fail(format!(
                "agents.command: agent `{}` command `{command}` contains / but is not an absolute path",
                agent.name()
            ));
        }
    }

    fn check_env_name(&mut self, agent: &Agent, name: &str) {
        let reason = if !Self::is_variable_name(name) {
            Some("is not a plain variable name")
        } else if Agent::is_baseline_variable(name) {
            Some("already reaches every agent through the baseline")
        } else if Self::LAUNCHER_VARIABLES.contains(&name) {
            Some("is set by the launcher from its own pane")
        } else if Self::TMUX_UPDATED_VARIABLES.contains(&name) {
            Some("is one tmux updates for every session, so the session would lose it")
        } else {
            None
        };
        if let Some(reason) = reason {
            self.fail(format!(
                "agents.env: agent `{}` entry `{name}` {reason}",
                agent.name()
            ));
        }
    }

    fn is_variable_name(name: &str) -> bool {
        let mut characters = name.chars();
        characters
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
            && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::domain::repo_draft::RepoDraft;

    struct ChecksFixture;

    impl ChecksFixture {
        fn draft() -> ConfigDraft {
            ConfigDraft::new(Path::new("/data/ringleader/worktrees"))
        }

        fn failures(draft: &ConfigDraft) -> Vec<String> {
            ConfigChecks::new(draft).failures()
        }

        fn fails_on(draft: ConfigDraft, key: &str) {
            let failures = Self::failures(&draft);
            assert_eq!(failures.len(), 1, "{failures:?}");
            assert!(failures[0].starts_with(key), "{failures:?}");
        }

        fn agent_with_env(entry: &str) -> ConfigDraft {
            Self::draft().agent(
                Agent::new("example-agent", "example-agent").with_env_names(&[entry.to_string()]),
            )
        }
    }

    #[test]
    fn the_defaults_pass_every_check() {
        assert!(ChecksFixture::failures(&ChecksFixture::draft()).is_empty());
    }

    #[test]
    fn a_tmux_prefix_of_two_dashes_fails() {
        ChecksFixture::fails_on(ChecksFixture::draft().tmux_prefix("--"), "tmux_prefix");
    }

    #[test]
    fn a_tmux_prefix_starting_with_a_dash_fails() {
        ChecksFixture::fails_on(ChecksFixture::draft().tmux_prefix("-x-"), "tmux_prefix");
    }

    #[test]
    fn a_branch_prefix_starting_with_a_dash_fails() {
        ChecksFixture::fails_on(
            ChecksFixture::draft().branch_prefix("-rl/"),
            "branch_prefix",
        );
    }

    #[test]
    fn a_short_branch_prefix_passes() {
        assert!(ChecksFixture::failures(&ChecksFixture::draft().branch_prefix("a-/")).is_empty());
    }

    #[test]
    fn malformed_and_out_of_range_session_sizes_fail() {
        for size in ["0x0", "200X50", "200 x 50", "10000x10000"] {
            ChecksFixture::fails_on(ChecksFixture::draft().session_size(size), "session_size");
        }
    }

    #[test]
    fn the_default_session_size_passes() {
        assert!(ChecksFixture::failures(&ChecksFixture::draft().session_size("200x50")).is_empty());
    }

    #[test]
    fn an_idle_threshold_below_two_seconds_fails() {
        for seconds in [0, 1] {
            ChecksFixture::fails_on(
                ChecksFixture::draft().activity_idle_seconds(seconds),
                "activity_idle_seconds",
            );
        }
    }

    #[test]
    fn env_entries_that_are_not_plain_or_belong_elsewhere_fail() {
        for entry in [
            "AWS_*",
            "FOO=bar",
            "",
            "TERM",
            "GPG_TTY",
            "PATH",
            "SSH_ASKPASS",
        ] {
            ChecksFixture::fails_on(ChecksFixture::agent_with_env(entry), "agents.env");
        }
    }

    #[test]
    fn an_empty_command_fails() {
        ChecksFixture::fails_on(
            ChecksFixture::draft().agent(Agent::new("example-agent", "")),
            "agents.command",
        );
    }

    #[test]
    fn a_relative_command_with_a_slash_fails() {
        ChecksFixture::fails_on(
            ChecksFixture::draft().agent(Agent::new("example-agent", "bin/agent")),
            "agents.command",
        );
    }

    #[test]
    fn a_repo_name_with_a_slash_fails() {
        let draft = ChecksFixture::draft().repo(RepoDraft::new(Path::new("/repos/x")).named("a/b"));

        ChecksFixture::fails_on(draft, "repos.name");
    }

    #[test]
    fn two_repos_with_one_name_fail() {
        let draft = ChecksFixture::draft()
            .repo(RepoDraft::new(Path::new("/repos/one")).named("example-project"))
            .repo(RepoDraft::new(Path::new("/repos/two")).named("example-project"));

        ChecksFixture::fails_on(draft, "repos.name");
    }

    #[test]
    fn two_agents_with_one_name_fail() {
        let draft = ChecksFixture::draft()
            .agent(Agent::new("claude", "claude"))
            .agent(Agent::new("claude", "claude"));

        ChecksFixture::fails_on(draft, "agents.name");
    }

    #[test]
    fn a_relative_worktree_root_fails() {
        ChecksFixture::fails_on(
            ChecksFixture::draft().worktree_root(Path::new("worktrees")),
            "worktree_root",
        );
    }

    #[test]
    fn a_variable_in_the_worktree_root_says_variables_are_not_expanded() {
        let failures = ChecksFixture::failures(
            &ChecksFixture::draft().worktree_root(&PathBuf::from("$HOME/x")),
        );

        assert!(failures[0].contains("environment variables are not expanded"));
    }

    #[test]
    fn a_relative_repo_path_fails() {
        let draft = ChecksFixture::draft().repo(RepoDraft::new(Path::new("projects/x")).named("x"));

        ChecksFixture::fails_on(draft, "repos.path");
    }

    #[test]
    fn two_bad_values_are_both_reported() {
        let draft = ChecksFixture::draft().tmux_prefix("--").session_size("0x0");

        assert_eq!(ChecksFixture::failures(&draft).len(), 2);
    }
}
