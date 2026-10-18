use std::iter::once;
use std::path::Path;

use super::agent::Agent;
use super::config_draft::ConfigDraft;
use super::hot_key::HotKey;
use super::key_bindings::KeyBindings;
use crate::domain::RepoName;

pub struct ConfigChecks<'a> {
    draft: &'a ConfigDraft,
    failures: Vec<String>,
}

impl<'a> ConfigChecks<'a> {
    const WIDTHS: (u16, u16) = (20, 500);
    const HEIGHTS: (u16, u16) = (10, 200);
    const IDLE_SECONDS: (i64, i64) = (2, 3600);
    const LAUNCHER_VARIABLES: [&'static str; 4] = ["TERM", "TMUX", "TMUX_PANE", "GPG_TTY"];
    const TMUX_UPDATED_VARIABLES: [&'static str; 5] = [
        "MSYSTEM",
        "SSH_ASKPASS",
        "SSH_AGENT_PID",
        "SSH_CONNECTION",
        "WINDOWID",
    ];
    const COMMAND_SEARCH_VARIABLE: &'static str = "PATH";

    pub fn new(draft: &'a ConfigDraft) -> Self {
        Self {
            draft,
            failures: Vec::new(),
        }
    }

    pub fn failures(mut self) -> Vec<String> {
        self.check_absolute("worktree_root", self.draft.raw_worktree_root());
        self.check_keys();
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

    fn check_keys(&mut self) {
        let settings = self.key_settings();
        for (setting, raw) in &settings {
            self.check_key(setting, raw);
        }
        self.check_repeated_keys(&settings);
    }

    fn key_settings(&self) -> Vec<(&'static str, String)> {
        let draft = self.draft;
        let listed = |setting: &'static str, keys: Vec<String>| {
            keys.into_iter().map(move |key| (setting, key))
        };
        let return_key = draft.raw_return_key().to_string();
        once((KeyBindings::RETURN_KEY_SETTING, return_key))
            .chain(listed(
                KeyBindings::NEXT_SESSION_SETTING,
                draft.raw_next_session_keys(),
            ))
            .chain(listed(
                KeyBindings::PREVIOUS_SESSION_SETTING,
                draft.raw_previous_session_keys(),
            ))
            .chain(listed(
                KeyBindings::SHELL_TOGGLE_SETTING,
                draft.raw_shell_toggle_keys(),
            ))
            .collect()
    }

    fn check_key(&mut self, setting: &str, raw: &str) {
        if HotKey::parse(raw).is_none() {
            self.fail(format!(
                "{setting}: `{}` must be a key a terminal can send, such as C-q, M-x, or F12: optional C-, M-, and S- modifiers, then a key name or one printable character other than a quote or \\. After C- the character must be a letter other than i or m, which terminals send as Tab and Enter, or ], ^, or _. S- cannot precede a character, and C- and S- cannot precede Enter, Tab, BTab, Escape, BSpace, or Space, except C-Space",
                raw.escape_default()
            ));
        }
    }

    fn check_repeated_keys(&mut self, settings: &[(&str, String)]) {
        let mut seen: Vec<(&str, HotKey)> = Vec::new();
        for (setting, raw) in settings {
            let Some(key) = HotKey::parse(raw) else {
                continue;
            };
            match seen.iter().find(|(_, earlier)| *earlier == key) {
                Some((earlier_setting, _)) => {
                    self.fail(Self::repeated_key_failure(setting, raw, earlier_setting))
                }
                None => seen.push((setting, key)),
            }
        }
    }

    fn repeated_key_failure(setting: &str, raw: &str, earlier_setting: &str) -> String {
        match setting == earlier_setting {
            true => format!("{setting}: `{raw}` is listed twice"),
            false => format!(
                "{setting}: `{raw}` is already bound in {earlier_setting}, and each key can do only one thing"
            ),
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
            for (name, value) in agent.env_assignments() {
                self.check_env_assignment(agent, name, value);
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
        let reason = Self::variable_name_refusal(name).or_else(|| Self::pass_through_refusal(name));
        if let Some(reason) = reason {
            self.fail(format!(
                "agents.env: agent `{}` entry `{name}` {reason}",
                agent.name()
            ));
        }
    }

    fn check_env_assignment(&mut self, agent: &Agent, name: &str, value: &str) {
        let reason = Self::variable_name_refusal(name)
            .or_else(|| Self::assignment_refusal(agent, name, value));
        if let Some(reason) = reason {
            self.fail(format!(
                "agents.set_env: agent `{}` entry `{name}` {reason}",
                agent.name()
            ));
        }
    }

    fn variable_name_refusal(name: &str) -> Option<&'static str> {
        if !Self::is_variable_name(name) {
            Some("is not a plain variable name")
        } else if Self::LAUNCHER_VARIABLES.contains(&name) {
            Some("is set by the launcher from its own pane")
        } else if Agent::is_true_color_variable(name) {
            Some("is set by the launcher, since Ringleader's tmux server renders true color")
        } else {
            None
        }
    }

    fn pass_through_refusal(name: &str) -> Option<&'static str> {
        if Agent::is_baseline_variable(name) {
            Some("already reaches every agent through the baseline")
        } else if Self::TMUX_UPDATED_VARIABLES.contains(&name) {
            Some("is one tmux updates for every session, so the session would lose it")
        } else {
            None
        }
    }

    fn assignment_refusal(agent: &Agent, name: &str, value: &str) -> Option<&'static str> {
        if name == Self::COMMAND_SEARCH_VARIABLE {
            Some("decides where the command is found, give the command an absolute path instead")
        } else if agent.env_names().iter().any(|listed| listed == name) {
            Some("is also listed in env, keep it in one of the two")
        } else if value.starts_with('$') {
            Some(
                "has a value starting with $, environment variables are not expanded, write the value out or start it with ~/",
            )
        } else if value.contains('\0') {
            Some("has a value containing a NUL byte")
        } else {
            None
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
    use crate::domain::Config;
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

        fn agent_setting(name: &str, value: &str) -> ConfigDraft {
            Self::draft().agent(Self::example_agent_setting(name, value))
        }

        fn example_agent_setting(name: &str, value: &str) -> Agent {
            Agent::new("example-agent", "example-agent")
                .with_env_assignments(&[(name.to_string(), value.to_string())])
        }
    }

    #[test]
    fn the_defaults_pass_every_check() {
        assert!(ChecksFixture::failures(&ChecksFixture::draft()).is_empty());
    }

    #[test]
    fn return_keys_a_terminal_can_send_pass() {
        for key in ["C-q", "M-x", "F12", "~", "}", "M-}"] {
            let draft = ChecksFixture::draft().return_key(key);

            assert!(
                ChecksFixture::failures(&draft).is_empty(),
                "{key} should pass"
            );
        }
    }

    #[test]
    fn return_keys_no_keystroke_reaches_fail_with_their_key_named() {
        for key in [
            "", "C-", "'", "\\", " ", "\n", "C-}", "C-1", "S-x", "C-Enter", "C-m",
        ] {
            ChecksFixture::fails_on(ChecksFixture::draft().return_key(key), "return_key");
        }
    }

    #[test]
    fn the_return_key_defaults_to_ctrl_q() {
        let config = Config::new(&ChecksFixture::draft()).unwrap();

        assert_eq!(config.key_bindings().return_key().to_string(), "C-q");
    }

    #[test]
    fn next_and_previous_keys_no_keystroke_reaches_fail_with_their_setting_named() {
        for key in ["", "C-", "'", "C-}", "S-x", "C-m"] {
            let next = ChecksFixture::draft().next_session_keys(&[key]);
            let previous = ChecksFixture::draft().previous_session_keys(&[key]);

            ChecksFixture::fails_on(next, "next_session_keys");
            ChecksFixture::fails_on(previous, "previous_session_keys");
        }
    }

    #[test]
    fn a_key_bound_twice_fails_with_the_later_setting_named() {
        for (draft, setting) in [
            (
                ChecksFixture::draft().next_session_keys(&["C-q"]),
                "next_session_keys",
            ),
            (
                ChecksFixture::draft().previous_session_keys(&["C-Q"]),
                "previous_session_keys",
            ),
            (
                ChecksFixture::draft()
                    .next_session_keys(&["M-Down"])
                    .previous_session_keys(&["M-Down"]),
                "previous_session_keys",
            ),
            (
                ChecksFixture::draft().next_session_keys(&["M-j", "M-j"]),
                "next_session_keys",
            ),
            (
                ChecksFixture::draft()
                    .return_key("C-M-x")
                    .next_session_keys(&["M-C-x"]),
                "next_session_keys",
            ),
            (
                ChecksFixture::draft()
                    .next_session_keys(&["PageDown"])
                    .previous_session_keys(&["NPage"]),
                "previous_session_keys",
            ),
        ] {
            ChecksFixture::fails_on(draft, setting);
        }
    }

    #[test]
    fn a_written_key_takes_itself_out_of_a_default_list_it_would_repeat() {
        let draft = ChecksFixture::draft().next_session_keys(&["M-k"]);

        let config = Config::new(&draft).unwrap();

        let previous = config.key_bindings().previous_session_keys();
        assert_eq!(previous, HotKey::parse_valid(["M-Up"]));
    }

    #[test]
    fn a_key_listed_twice_in_one_setting_is_named_as_listed_twice() {
        let draft = ChecksFixture::draft().next_session_keys(&["M-j", "M-Down", "M-j"]);

        let failures = ChecksFixture::failures(&draft);

        assert_eq!(failures, ["next_session_keys: `M-j` is listed twice"]);
    }

    #[test]
    fn empty_session_key_lists_pass() {
        let none: [&str; 0] = [];
        let draft = ChecksFixture::draft()
            .next_session_keys(&none)
            .previous_session_keys(&none);

        assert!(ChecksFixture::failures(&draft).is_empty());
    }

    #[test]
    fn shell_toggle_keys_no_keystroke_reaches_fail_with_their_setting_named() {
        for key in ["", "C-", "'", "C-}", "S-x", "C-m"] {
            let draft = ChecksFixture::draft().shell_toggle_keys(&[key]);

            ChecksFixture::fails_on(draft, "shell_toggle_keys");
        }
    }

    #[test]
    fn a_shell_toggle_key_listed_twice_is_named_as_listed_twice() {
        let draft = ChecksFixture::draft().shell_toggle_keys(&["M-l", "F8", "M-l"]);

        let failures = ChecksFixture::failures(&draft);

        assert_eq!(failures, ["shell_toggle_keys: `M-l` is listed twice"]);
    }

    #[test]
    fn a_shell_toggle_key_bound_by_another_key_setting_fails_naming_both_settings() {
        let written = ChecksFixture::draft()
            .return_key("C-q")
            .next_session_keys(&["M-j"])
            .previous_session_keys(&["M-Up"]);
        for (key, other_setting) in [
            ("C-q", "return_key"),
            ("M-j", "next_session_keys"),
            ("M-Up", "previous_session_keys"),
        ] {
            let draft = written.clone().shell_toggle_keys(&[key]);

            let failures = ChecksFixture::failures(&draft);

            assert_eq!(
                failures,
                [format!(
                    "shell_toggle_keys: `{key}` is already bound in {other_setting}, and each key can do only one thing"
                )]
            );
        }
    }

    #[test]
    fn a_missing_shell_toggle_list_keeps_alt_l() {
        let config = Config::new(&ChecksFixture::draft()).unwrap();

        let toggle = config.key_bindings().shell_toggle_keys();
        assert_eq!(toggle, HotKey::parse_valid(["M-l"]));
    }

    #[test]
    fn an_empty_shell_toggle_list_binds_no_toggle_key() {
        let none: [&str; 0] = [];
        let draft = ChecksFixture::draft().shell_toggle_keys(&none);

        let config = Config::new(&draft).unwrap();

        assert!(config.key_bindings().shell_toggle_keys().is_empty());
    }

    #[test]
    fn every_listed_shell_toggle_key_is_kept() {
        let draft = ChecksFixture::draft().shell_toggle_keys(&["M-o", "F8"]);

        let config = Config::new(&draft).unwrap();

        let toggle = config.key_bindings().shell_toggle_keys();
        assert_eq!(toggle, HotKey::parse_valid(["M-o", "F8"]));
    }

    #[test]
    fn a_written_shell_toggle_key_takes_itself_out_of_a_default_session_list() {
        let draft = ChecksFixture::draft().shell_toggle_keys(&["M-j"]);

        let config = Config::new(&draft).unwrap();

        let next = config.key_bindings().next_session_keys();
        assert_eq!(next, HotKey::parse_valid(["M-Down"]));
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
            "COLORTERM",
            "CLAUDE_CODE_TMUX_TRUECOLOR",
            "PATH",
            "SSH_ASKPASS",
        ] {
            ChecksFixture::fails_on(ChecksFixture::agent_with_env(entry), "agents.env");
        }
    }

    #[test]
    fn set_env_entries_that_are_not_plain_or_belong_to_the_launcher_fail() {
        for name in [
            "AWS_*",
            "FOO=bar",
            "",
            "TERM",
            "GPG_TTY",
            "COLORTERM",
            "CLAUDE_CODE_TMUX_TRUECOLOR",
        ] {
            ChecksFixture::fails_on(
                ChecksFixture::agent_setting(name, "/accounts/example"),
                "agents.set_env",
            );
        }
    }

    #[test]
    fn a_true_color_variable_in_set_env_is_refused_with_the_launcher_named() {
        let draft = ChecksFixture::agent_setting("COLORTERM", "truecolor");

        assert_eq!(
            ChecksFixture::failures(&draft),
            [
                "agents.set_env: agent `example-agent` entry `COLORTERM` is set by the launcher, since Ringleader's tmux server renders true color"
            ]
        );
    }

    #[test]
    fn a_set_env_value_with_a_nul_byte_fails() {
        ChecksFixture::fails_on(
            ChecksFixture::agent_setting("EXAMPLE_ACCOUNT_DIR", "/accounts\0/example"),
            "agents.set_env",
        );
    }

    #[test]
    fn a_set_env_value_starting_with_a_variable_fails() {
        for value in ["$HOME/.example-account", "${HOME}/.example-account"] {
            ChecksFixture::fails_on(
                ChecksFixture::agent_setting("EXAMPLE_ACCOUNT_DIR", value),
                "agents.set_env",
            );
        }
    }

    #[test]
    fn setting_path_fails_because_the_command_lookup_would_disagree() {
        ChecksFixture::fails_on(
            ChecksFixture::agent_setting("PATH", "/opt/example/bin"),
            "agents.set_env",
        );
    }

    #[test]
    fn a_name_both_passed_through_and_set_fails() {
        let names = ["EXAMPLE_ACCOUNT_DIR".to_string()];
        let agent =
            ChecksFixture::example_agent_setting("EXAMPLE_ACCOUNT_DIR", "/accounts/example")
                .with_env_names(&names);

        ChecksFixture::fails_on(ChecksFixture::draft().agent(agent), "agents.set_env");
    }

    #[test]
    fn set_env_may_override_a_baseline_variable() {
        let draft = ChecksFixture::agent_setting("HOME", "/home/example-account");

        assert!(ChecksFixture::failures(&draft).is_empty());
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
        let draft = ChecksFixture::draft().return_key("C-").session_size("0x0");

        assert_eq!(ChecksFixture::failures(&draft).len(), 2);
    }
}
