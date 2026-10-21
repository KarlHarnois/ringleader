use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, value};

use super::config_file_writer::ConfigFileWriter;
use super::raw_config::RawConfig;
use crate::application::ports::{ConfigError, ConfigSource};
use crate::domain::{Config, ConfigDraft, Repo, RepoName};

pub struct TomlConfigSource {
    path: PathBuf,
    default_worktree_root: PathBuf,
    home: Option<PathBuf>,
}

impl TomlConfigSource {
    pub const PATH_VARIABLE: &'static str = "RINGLEADER_CONFIG";
    pub const TEMPLATE: &'static str = r#"# Absolute or ~/ paths only. Environment variables are not expanded.
# Defaults to $XDG_DATA_HOME/ringleader/worktrees.
# worktree_root = "~/.local/share/ringleader/worktrees"
# The key that detaches from a session back to Ringleader. Sessions have no
# prefix key, so this is the only way back. Inside your own tmux, it must not be
# your tmux's prefix or bound in its root table.
return_key = "C-q"
# Keys that switch to the next or previous session in name order, wrapping at
# either end. [] turns a direction off, and a missing list keeps these defaults.
# Inside your own tmux, they must not be bound in its root table, and its
# escape-time must be low, such as 10, or Esc followed by j, k, or an arrow
# switches sessions. A removed key that tmux binds in copy mode, other than
# M-Up, M-Down, and C-k, stays unbound there until Ringleader's tmux server
# restarts.
next_session_keys = ["M-j", "M-Down"]
previous_session_keys = ["M-k", "M-Up"]
# Keys that switch between a session's agent and its shell, the one t opens,
# while you are attached. [] turns the toggle off, and a missing list keeps
# this default. They follow the rules of the session keys inside your own tmux
# and in copy mode, so a removed M-l stays unbound in emacs copy mode until
# Ringleader's tmux server restarts.
shell_toggle_keys = ["M-l"]
# Keys that open the session finder over a session while you are attached.
# Enter switches to the session you pick, and Esc keeps you where you were.
# [] turns the finder off, and a missing list keeps this default. C-k takes
# Ctrl-K, which deletes to the end of the line, away from agents and shells.
# Inside your own tmux, they must not be bound in its root table.
finder_keys = ["C-k"]
state_poll_ms = 1000
preview_refresh_ms = 750
diff_refresh_ms = 3000
pull_request_refresh_ms = 30000
activity_idle_seconds = 3
session_size = "200x50"
# Ring the terminal bell when an agent finishes working and waits for input.
# It stays quiet for an agent whose window a client is showing. It rings only
# while the Ringleader TUI shows, never while you are attached to a session.
bell_on_waiting = false
scan_depth = 3

# Optional. Repos found by scanning the launch directory need no entry here.
# Use [[repos]] only to pin a custom name, a custom base_branch, or a repo
# that should always appear in the picker. Explicit entries win on path collision.
# [[repos]]
# name = "example-project"
# path = "~/projects/example-project"
# base_branch = "main"

# The first agent is the default for new sessions.
[[agents]]
name = "claude"
command = "claude"
# command is never split. Put flags in args. {name} is the session name.
args = ["--name", "{name}"]
env = ["ANTHROPIC_API_KEY"]
# Agents start with a fixed baseline such as PATH and HOME. env copies more
# variables from Ringleader's environment. set_env sets fixed values, for
# example the config directory of a second Claude account. Values are used as
# written: only a leading ~/ expands. Keep secrets in env.
# set_env = { CLAUDE_CONFIG_DIR = "~/.claude-work" }
prompt_args = ["{prompt}"]
# Added after args when a new session starts in plan mode. The new session
# dialog opens in plan mode when the first agent has plan_args, and Alt-j
# switches modes. An agent without plan_args has no plan mode.
plan_args = ["--permission-mode", "plan"]
# Added after args when restart recreates a stopped session, such as after a
# reboot, to resume its conversation. An exited agent restarts without them,
# so a resume that finds no conversation and exits restarts fresh. A session
# in a repo's main checkout always restarts without them, since your own
# conversations share that directory.
resume_args = ["--continue"]
# Lines typed into the agent once its prompt is ready, each followed by Enter.
# startup_input is typed at every start, rename_input after a rename.
startup_input = ["/color"]
rename_input = ["/rename {name}"]
# Ringleader gives claude and opencode its built-in skills, such as one that
# runs the ringleader CLI from inside a session. built_in_skills = false turns
# all of them off, for an organization whose Claude Code policy blocks plugin
# loading, since claude then refuses to start.
# built_in_skills = false

[[agents]]
name = "opencode"
command = "opencode"
env = ["OPENAI_API_KEY"]
prompt_args = ["--prompt={prompt}"]
plan_args = ["--agent", "plan"]
resume_args = ["--continue"]
"#;
    const REPOS_KEY: &'static str = "repos";

    pub fn new(path: &Path, default_worktree_root: &Path, home: Option<&Path>) -> Self {
        Self {
            path: path.to_path_buf(),
            default_worktree_root: default_worktree_root.to_path_buf(),
            home: home.map(Path::to_path_buf),
        }
    }

    pub fn resolved_from(self, launch_directory: &Path) -> Self {
        Self {
            path: launch_directory.join(&self.path),
            ..self
        }
    }

    pub fn defaults(&self) -> Config {
        Config::defaults(&self.default_worktree_root)
    }

    pub fn exists(&self) -> bool {
        fs::symlink_metadata(&self.path).is_ok()
    }

    pub fn write_first_run_template(&self) -> Result<(), ConfigError> {
        let directory = self.path.parent().unwrap_or(Path::new("/"));
        fs::create_dir_all(directory).map_err(|error| self.io_error(error))?;
        let canonical = fs::canonicalize(directory).map_err(|error| self.io_error(error))?;
        if canonical.starts_with("/nix/store") {
            return Err(ConfigError::NixManaged {
                path: self.path.clone(),
            });
        }
        let target = canonical.join(self.path.file_name().unwrap_or_default());
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|error| self.io_error(error))?;
        file.write_all(Self::TEMPLATE.as_bytes())
            .map_err(|error| self.io_error(error))
    }

    fn parse(&self, text: &str) -> Result<Config, ConfigError> {
        let raw = RawConfig::parse(text).map_err(|detail| ConfigError::Parse {
            path: self.path.clone(),
            detail,
        })?;
        let draft = raw.into_draft(
            ConfigDraft::new(&self.default_worktree_root),
            self.home.as_deref(),
        );
        Config::new(&draft).map_err(|failures| ConfigError::Invalid {
            path: self.path.clone(),
            failures,
        })
    }

    fn edit(
        &self,
        change: impl FnOnce(&mut DocumentMut) -> Result<(), ConfigError>,
    ) -> Result<(), ConfigError> {
        let writer = ConfigFileWriter::open(&self.path)?;
        let text = writer.text()?;
        self.parse(&text)?;
        let mut document: DocumentMut =
            text.parse()
                .map_err(|error: toml_edit::TomlError| ConfigError::Parse {
                    path: self.path.clone(),
                    detail: error.to_string(),
                })?;
        change(&mut document)?;
        let edited = document.to_string();
        self.parse(&edited)?;
        writer.replace(&edited)
    }

    fn repos_table<'d>(
        &self,
        document: &'d mut DocumentMut,
    ) -> Result<&'d mut ArrayOfTables, ConfigError> {
        let item = document
            .entry(Self::REPOS_KEY)
            .or_insert(Item::ArrayOfTables(ArrayOfTables::new()));
        item.as_array_of_tables_mut()
            .ok_or_else(|| ConfigError::Parse {
                path: self.path.clone(),
                detail: "repos is not a list of [[repos]] tables".to_string(),
            })
    }

    fn io_error(&self, error: io::Error) -> ConfigError {
        ConfigError::Io {
            path: self.path.clone(),
            detail: error.to_string(),
        }
    }
}

impl ConfigSource for TomlConfigSource {
    fn path(&self) -> PathBuf {
        self.path.clone()
    }

    fn load(&self) -> Result<Config, ConfigError> {
        match fs::read_to_string(&self.path) {
            Ok(text) => self.parse(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(self.defaults()),
            Err(error) => Err(self.io_error(error)),
        }
    }

    fn add_repo(&self, repo: &Repo) -> Result<(), ConfigError> {
        self.edit(|document| {
            let mut table = Table::new();
            table["name"] = value(repo.name().as_str());
            table["path"] = value(repo.path().to_string_lossy().as_ref());
            if let Some(base_branch) = repo.base_branch() {
                table["base_branch"] = value(base_branch.as_str());
            }
            self.repos_table(document)?.push(table);
            Ok(())
        })
    }

    fn remove_repo(&self, name: &RepoName) -> Result<(), ConfigError> {
        self.edit(|document| {
            let repos = self.repos_table(document)?;
            let before = repos.len();
            repos.retain(|table| table.get("name").and_then(Item::as_str) != Some(name.as_str()));
            if repos.len() == before {
                return Err(ConfigError::UnknownRepo {
                    path: self.path.clone(),
                    name: name.to_string(),
                });
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Agent, AgentMode, HotKey, KeyBindings, SessionName};
    use crate::test_support::InheritedPath;

    const FIXTURE: &str = include_str!("../../../tests/fixtures/config.toml");

    struct SourceFixture {
        directory: tempfile::TempDir,
    }

    impl SourceFixture {
        fn new() -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
            }
        }

        fn path(&self) -> PathBuf {
            self.directory.path().join("config.toml")
        }

        fn source(&self) -> TomlConfigSource {
            TomlConfigSource::new(
                &self.path(),
                Path::new("/data/ringleader/worktrees"),
                Some(Path::new("/home/example")),
            )
        }

        fn other_repo() -> Repo {
            Repo::explicit(
                RepoName::parse("other").unwrap(),
                Path::new("/repos/other"),
                None,
            )
        }

        fn store_file() -> PathBuf {
            std::env::split_paths(&InheritedPath::value())
                .map(|directory| directory.join("tmux"))
                .filter_map(|candidate| fs::canonicalize(candidate).ok())
                .find(|candidate| candidate.starts_with("/nix/store"))
                .expect("a tmux from the nix store")
        }

        fn with_text(text: &str) -> Self {
            let fixture = Self::new();
            fs::write(fixture.path(), text).unwrap();
            fixture
        }

        fn names(keys: &[HotKey]) -> Vec<String> {
            keys.iter().map(HotKey::to_string).collect()
        }
    }

    #[test]
    fn a_relative_config_path_resolves_from_the_launch_directory() {
        let source = TomlConfigSource::new(
            Path::new("configs/ringleader.toml"),
            Path::new("/data/ringleader/worktrees"),
            None,
        )
        .resolved_from(Path::new("/home/example/code"));

        assert_eq!(
            source.path(),
            Path::new("/home/example/code/configs/ringleader.toml")
        );
    }

    #[test]
    fn an_absolute_config_path_stays_as_given() {
        let source = TomlConfigSource::new(
            Path::new("/config/ringleader.toml"),
            Path::new("/data/ringleader/worktrees"),
            None,
        )
        .resolved_from(Path::new("/home/example/code"));

        assert_eq!(source.path(), Path::new("/config/ringleader.toml"));
    }

    #[test]
    fn a_missing_config_loads_the_defaults_without_writing_a_file() {
        let fixture = SourceFixture::new();

        let config = fixture.source().load().unwrap();

        assert_eq!(
            config,
            Config::defaults(Path::new("/data/ringleader/worktrees"))
        );
        assert!(!fixture.path().exists());
    }

    #[test]
    fn the_fixture_config_loads_its_agents_and_repos() {
        let fixture = SourceFixture::with_text(FIXTURE);

        let config = fixture.source().load().unwrap();

        let agents: Vec<&str> = config.agents().iter().map(Agent::name).collect();
        assert_eq!(agents, ["claude", "opencode"]);
        assert_eq!(config.repos()[0].name().as_str(), "example-project");
        assert_eq!(config.session_size().width(), 160);
    }

    #[test]
    fn the_session_keys_in_a_file_reach_the_config() {
        let fixture = SourceFixture::with_text(FIXTURE);

        let config = fixture.source().load().unwrap();

        let bindings = config.key_bindings();
        assert_eq!(SourceFixture::names(bindings.next_session_keys()), ["M-n"]);
        assert_eq!(
            SourceFixture::names(bindings.previous_session_keys()),
            ["M-p", "F7"]
        );
    }

    #[test]
    fn the_shell_toggle_keys_in_a_file_reach_the_config() {
        let fixture = SourceFixture::with_text("shell_toggle_keys = [\"M-o\", \"F8\"]\n");

        let config = fixture.source().load().unwrap();

        let toggle = config.key_bindings().shell_toggle_keys();
        assert_eq!(SourceFixture::names(toggle), ["M-o", "F8"]);
    }

    #[test]
    fn an_empty_shell_toggle_list_in_a_file_binds_no_toggle_key() {
        let fixture = SourceFixture::with_text("shell_toggle_keys = []\n");

        let config = fixture.source().load().unwrap();

        assert!(config.key_bindings().shell_toggle_keys().is_empty());
    }

    #[test]
    fn a_file_without_shell_toggle_keys_keeps_alt_l() {
        let fixture = SourceFixture::with_text("return_key = \"C-q\"\n");

        let config = fixture.source().load().unwrap();

        let toggle = config.key_bindings().shell_toggle_keys();
        assert_eq!(SourceFixture::names(toggle), ["M-l"]);
    }

    #[test]
    fn the_finder_keys_in_a_file_reach_the_config() {
        let fixture = SourceFixture::with_text("finder_keys = [\"M-f\", \"F9\"]\n");

        let config = fixture.source().load().unwrap();

        let finder = config.key_bindings().finder_keys();
        assert_eq!(SourceFixture::names(finder), ["M-f", "F9"]);
    }

    #[test]
    fn an_empty_finder_list_in_a_file_binds_no_finder_key() {
        let fixture = SourceFixture::with_text("finder_keys = []\n");

        let config = fixture.source().load().unwrap();

        assert!(config.key_bindings().finder_keys().is_empty());
    }

    #[test]
    fn an_older_config_whose_return_key_is_a_default_session_key_still_loads() {
        let fixture = SourceFixture::with_text("return_key = \"M-k\"\n");

        let config = fixture.source().load().unwrap();

        let bindings = config.key_bindings();
        assert_eq!(bindings.return_key().to_string(), "M-k");
        assert_eq!(
            SourceFixture::names(bindings.previous_session_keys()),
            ["M-Up"]
        );
    }

    #[test]
    fn the_first_run_template_loads_the_default_key_bindings() {
        let fixture = SourceFixture::new();

        fixture.source().write_first_run_template().unwrap();

        assert_eq!(
            fixture.source().load().unwrap().key_bindings(),
            &KeyBindings::default()
        );
    }

    #[test]
    fn a_config_without_agents_loads_with_no_agents() {
        let fixture = SourceFixture::with_text("return_key = \"C-q\"\n");

        assert!(fixture.source().load().unwrap().agents().is_empty());
    }

    #[test]
    fn a_leftover_branch_prefix_key_is_ignored() {
        let fixture = SourceFixture::with_text("branch_prefix = \"ringleader/\"\n");

        assert!(fixture.source().load().is_ok());
    }

    #[test]
    fn an_empty_config_loads() {
        let fixture = SourceFixture::with_text("");

        assert!(fixture.source().load().is_ok());
    }

    #[test]
    fn unparsable_text_is_a_parse_error() {
        let fixture = SourceFixture::with_text("branch_prefix = ");

        assert!(matches!(
            fixture.source().load(),
            Err(ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn an_old_tmux_prefix_line_loads_and_changes_nothing() {
        let fixture = SourceFixture::with_text("tmux_prefix = \"agents-\"\n");

        let config = fixture.source().load().unwrap();

        assert_eq!(config.naming().tmux_prefix(), "ringleader-");
    }

    #[test]
    fn a_tilde_root_and_a_tilde_repo_path_expand_from_home() {
        let fixture = SourceFixture::with_text(
            "worktree_root = \"~\"\n[[repos]]\nname = \"x\"\npath = \"~/projects/x\"\n",
        );

        let config = fixture.source().load().unwrap();

        assert_eq!(config.naming().worktree_root(), Path::new("/home/example"));
        assert_eq!(
            config.repos()[0].path(),
            Path::new("/home/example/projects/x")
        );
    }

    #[test]
    fn a_tilde_agent_command_expands_from_home() {
        let fixture =
            SourceFixture::with_text("[[agents]]\nname = \"a\"\ncommand = \"~/bin/agent\"\n");

        let config = fixture.source().load().unwrap();

        assert_eq!(config.agents()[0].command(), "/home/example/bin/agent");
    }

    #[test]
    fn an_agent_set_env_loads_in_name_order_with_tilde_values_expanded() {
        let fixture = SourceFixture::with_text(concat!(
            "[[agents]]\nname = \"a\"\ncommand = \"a\"\n",
            "set_env = { EXAMPLE_MODE = \"plain\", EXAMPLE_ACCOUNT_DIR = \"~/accounts/example\" }\n",
        ));

        let config = fixture.source().load().unwrap();

        assert_eq!(
            config.agents()[0].env_assignments(),
            [
                (
                    "EXAMPLE_ACCOUNT_DIR".to_string(),
                    "/home/example/accounts/example".to_string()
                ),
                ("EXAMPLE_MODE".to_string(), "plain".to_string()),
            ]
        );
    }

    #[test]
    fn agent_input_loads_with_the_name_placeholder_kept_for_each_session() {
        let fixture = SourceFixture::with_text(concat!(
            "[[agents]]\nname = \"a\"\ncommand = \"a\"\n",
            "startup_input = [\"/color\"]\nrename_input = [\"/rename {name}\"]\n",
        ));

        let config = fixture.source().load().unwrap();

        let session = SessionName::new("fix-auth").unwrap();
        assert_eq!(config.agents()[0].startup_input(&session), ["/color"]);
        assert_eq!(
            config.agents()[0].rename_input(&session),
            ["/rename fix-auth"]
        );
    }

    #[test]
    fn agent_plan_args_load_for_plan_mode() {
        let fixture = SourceFixture::with_text(concat!(
            "[[agents]]\nname = \"a\"\ncommand = \"a\"\n",
            "plan_args = [\"--mode\", \"plan\"]\n",
        ));

        let config = fixture.source().load().unwrap();

        let session = SessionName::new("fix-auth").unwrap();
        assert_eq!(
            config.agents()[0].launch_argv(&session, AgentMode::Plan, ""),
            ["a", "--mode", "plan"]
        );
    }

    #[test]
    fn an_agent_without_plan_args_loads_with_no_plan_mode() {
        let fixture = SourceFixture::with_text("[[agents]]\nname = \"a\"\ncommand = \"a\"\n");

        let config = fixture.source().load().unwrap();

        assert!(
            config.agents()[0]
                .launch_refusal(AgentMode::Plan, "")
                .is_some()
        );
    }

    #[test]
    fn agent_resume_args_load_for_a_resumed_restart() {
        let fixture = SourceFixture::with_text(concat!(
            "[[agents]]\nname = \"a\"\ncommand = \"a\"\n",
            "resume_args = [\"--continue\"]\n",
        ));

        let config = fixture.source().load().unwrap();

        let session = SessionName::new("fix-auth").unwrap();
        assert_eq!(
            config.agents()[0].resume_argv(&session),
            ["a", "--continue"]
        );
    }

    #[test]
    fn an_agent_without_resume_args_loads_to_resume_as_a_plain_restart() {
        let fixture = SourceFixture::with_text("[[agents]]\nname = \"a\"\ncommand = \"a\"\n");

        let config = fixture.source().load().unwrap();

        let session = SessionName::new("fix-auth").unwrap();
        assert_eq!(config.agents()[0].resume_argv(&session), ["a"]);
    }

    #[test]
    fn an_agent_without_input_loads_with_none() {
        let fixture = SourceFixture::with_text("[[agents]]\nname = \"a\"\ncommand = \"a\"\n");

        let config = fixture.source().load().unwrap();

        let session = SessionName::new("fix-auth").unwrap();
        assert!(config.agents()[0].startup_input(&session).is_empty());
        assert!(config.agents()[0].rename_input(&session).is_empty());
    }

    #[test]
    fn unexpanded_and_relative_paths_fail_with_their_key_named() {
        for (text, key) in [
            ("worktree_root = \"worktrees\"\n", "worktree_root"),
            ("worktree_root = \"$HOME/x\"\n", "worktree_root"),
            ("worktree_root = \"~other/x\"\n", "worktree_root"),
            (
                "[[repos]]\nname = \"x\"\npath = \"projects/x\"\n",
                "repos.path",
            ),
        ] {
            let fixture = SourceFixture::with_text(text);

            let Err(ConfigError::Invalid { failures, .. }) = fixture.source().load() else {
                panic!("{text} should fail a load check");
            };
            assert!(failures[0].starts_with(key), "{failures:?}");
        }
    }

    #[test]
    fn a_fractional_idle_threshold_names_its_key() {
        let fixture = SourceFixture::with_text("activity_idle_seconds = 2.5\n");

        let error = fixture.source().load().unwrap_err();

        assert!(error.to_string().contains("activity_idle_seconds"));
    }

    #[test]
    fn the_first_run_template_is_written_verbatim_and_loads_both_agents() {
        let fixture = SourceFixture::new();

        fixture.source().write_first_run_template().unwrap();

        assert_eq!(
            fs::read_to_string(fixture.path()).unwrap(),
            TomlConfigSource::TEMPLATE
        );
        assert_eq!(fixture.source().load().unwrap().agents().len(), 2);
    }

    #[test]
    fn the_first_run_template_writes_the_default_shell_toggle_keys() {
        let written = TomlConfigSource::TEMPLATE
            .lines()
            .any(|line| line == r#"shell_toggle_keys = ["M-l"]"#);

        assert!(written);
    }

    #[test]
    fn the_first_run_template_writes_the_default_finder_keys() {
        let written = TomlConfigSource::TEMPLATE
            .lines()
            .any(|line| line == r#"finder_keys = ["C-k"]"#);

        assert!(written);
    }

    #[test]
    fn the_first_run_template_starts_claude_in_its_permission_plan_mode() {
        let fixture = SourceFixture::new();
        fixture.source().write_first_run_template().unwrap();

        let config = fixture.source().load().unwrap();

        let session = SessionName::new("fix-auth").unwrap();
        assert_eq!(
            config
                .agent("claude")
                .unwrap()
                .launch_argv(&session, AgentMode::Plan, ""),
            ["claude", "--name", "fix-auth", "--permission-mode", "plan"]
        );
    }

    #[test]
    fn the_first_run_template_gives_claude_the_built_in_skills() {
        let fixture = SourceFixture::new();
        fixture.source().write_first_run_template().unwrap();

        let config = fixture.source().load().unwrap();

        assert!(config.agent("claude").unwrap().gets_built_in_skills());
    }

    #[test]
    fn an_agent_set_to_skip_the_built_in_skills_goes_without_them() {
        let fixture = SourceFixture::with_text(
            "[[agents]]\nname = \"claude\"\ncommand = \"claude\"\nbuilt_in_skills = false\n",
        );

        let config = fixture.source().load().unwrap();

        assert!(!config.agent("claude").unwrap().gets_built_in_skills());
    }

    #[test]
    fn the_first_run_template_starts_opencode_in_its_plan_agent() {
        let fixture = SourceFixture::new();
        fixture.source().write_first_run_template().unwrap();

        let config = fixture.source().load().unwrap();

        let session = SessionName::new("fix-auth").unwrap();
        assert_eq!(
            config
                .agent("opencode")
                .unwrap()
                .launch_argv(&session, AgentMode::Plan, ""),
            ["opencode", "--agent", "plan"]
        );
    }

    #[test]
    fn the_first_run_template_resumes_claude_with_continue() {
        let fixture = SourceFixture::new();
        fixture.source().write_first_run_template().unwrap();

        let config = fixture.source().load().unwrap();

        let session = SessionName::new("fix-auth").unwrap();
        assert_eq!(
            config.agent("claude").unwrap().resume_argv(&session),
            ["claude", "--name", "fix-auth", "--continue"]
        );
    }

    #[test]
    fn the_first_run_template_resumes_opencode_with_continue() {
        let fixture = SourceFixture::new();
        fixture.source().write_first_run_template().unwrap();

        let config = fixture.source().load().unwrap();

        let session = SessionName::new("fix-auth").unwrap();
        assert_eq!(
            config.agent("opencode").unwrap().resume_argv(&session),
            ["opencode", "--continue"]
        );
    }

    #[test]
    fn the_template_never_overwrites_an_existing_file() {
        let fixture = SourceFixture::with_text(FIXTURE);

        assert!(fixture.source().write_first_run_template().is_err());
        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), FIXTURE);
    }

    #[test]
    fn adding_a_repo_appends_an_entry_and_keeps_comments() {
        let fixture = SourceFixture::new();
        fixture.source().write_first_run_template().unwrap();
        let repo = Repo::explicit(
            RepoName::parse("other").unwrap(),
            Path::new("/repos/other"),
            crate::domain::BaseBranch::new("develop"),
        );

        fixture.source().add_repo(&repo).unwrap();

        let text = fs::read_to_string(fixture.path()).unwrap();
        assert!(text.contains("# The first agent is the default for new sessions."));
        assert_eq!(
            fixture.source().load().unwrap().repos()[0].name().as_str(),
            "other"
        );
    }

    #[test]
    fn removing_a_repo_drops_its_entry() {
        let fixture = SourceFixture::with_text(FIXTURE);

        fixture
            .source()
            .remove_repo(&RepoName::parse("example-project").unwrap())
            .unwrap();

        assert!(fixture.source().load().unwrap().repos().is_empty());
    }

    #[test]
    fn removing_an_unknown_repo_is_refused() {
        let fixture = SourceFixture::with_text(FIXTURE);

        let error = fixture
            .source()
            .remove_repo(&RepoName::parse("nope").unwrap())
            .unwrap_err();

        assert!(matches!(error, ConfigError::UnknownRepo { .. }));
    }

    #[test]
    fn an_edit_that_would_fail_a_load_check_writes_nothing() {
        let fixture = SourceFixture::with_text(FIXTURE);
        let duplicate = Repo::explicit(
            RepoName::parse("example-project").unwrap(),
            Path::new("/repos/elsewhere"),
            None,
        );

        assert!(fixture.source().add_repo(&duplicate).is_err());
        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), FIXTURE);
    }

    #[test]
    fn an_unparsable_config_is_never_rewritten() {
        let fixture = SourceFixture::with_text("branch_prefix = ");

        assert!(
            fixture
                .source()
                .add_repo(&SourceFixture::other_repo())
                .is_err()
        );
        assert_eq!(
            fs::read_to_string(fixture.path()).unwrap(),
            "branch_prefix = "
        );
    }

    #[test]
    fn a_symlinked_config_stays_a_symlink_after_a_write() {
        let fixture = SourceFixture::new();
        let real = fixture.directory.path().join("real.toml");
        fs::write(&real, FIXTURE).unwrap();
        std::os::unix::fs::symlink(&real, fixture.path()).unwrap();

        fixture
            .source()
            .add_repo(&SourceFixture::other_repo())
            .unwrap();

        assert!(
            fs::symlink_metadata(fixture.path())
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(fs::read_to_string(&real).unwrap().contains("other"));
    }

    #[test]
    fn a_write_keeps_the_permission_bits() {
        use std::os::unix::fs::PermissionsExt;
        let fixture = SourceFixture::with_text(FIXTURE);
        fs::set_permissions(fixture.path(), fs::Permissions::from_mode(0o640)).unwrap();

        fixture
            .source()
            .add_repo(&SourceFixture::other_repo())
            .unwrap();

        let mode = fs::metadata(fixture.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
    }

    #[test]
    fn a_read_only_config_is_reported_and_left_unchanged() {
        use std::os::unix::fs::PermissionsExt;
        let fixture = SourceFixture::with_text(FIXTURE);
        fs::set_permissions(fixture.path(), fs::Permissions::from_mode(0o444)).unwrap();
        let writable_anyway = OpenOptions::new().write(true).open(fixture.path()).is_ok();

        let result = fixture.source().add_repo(&SourceFixture::other_repo());

        if !writable_anyway {
            assert!(matches!(result, Err(ConfigError::ReadOnly { .. })));
            assert_eq!(fs::read_to_string(fixture.path()).unwrap(), FIXTURE);
        }
    }

    #[test]
    fn a_config_inside_the_nix_store_is_reported_as_nix_managed() {
        let fixture = SourceFixture::new();
        let store_file = SourceFixture::store_file();
        std::os::unix::fs::symlink(&store_file, fixture.path()).unwrap();

        let error = fixture
            .source()
            .add_repo(&SourceFixture::other_repo())
            .unwrap_err();

        assert!(matches!(error, ConfigError::NixManaged { .. }));
    }
}
