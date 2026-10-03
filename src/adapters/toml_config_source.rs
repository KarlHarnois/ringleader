use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::application::ports::{ConfigError, ConfigSource};
use crate::domain::{Agent, Config, ConfigDraft, Repo, RepoDraft, RepoName};

pub struct TomlConfigSource {
    path: PathBuf,
    default_worktree_root: PathBuf,
}

impl TomlConfigSource {
    pub fn new(path: &Path, default_worktree_root: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            default_worktree_root: default_worktree_root.to_path_buf(),
        }
    }

    pub fn defaults(&self) -> Config {
        Config::defaults(&self.default_worktree_root)
    }

    fn parse(&self, text: &str) -> Result<Config, ConfigError> {
        let raw: RawConfig = toml::from_str(text).map_err(|error| ConfigError::Parse {
            path: self.path.clone(),
            detail: error.to_string(),
        })?;
        let draft = raw.into_draft(ConfigDraft::new(&self.default_worktree_root));
        Config::new(&draft).map_err(|failures| ConfigError::Invalid {
            path: self.path.clone(),
            failures,
        })
    }

    fn writes_unsupported(&self) -> ConfigError {
        ConfigError::Io {
            path: self.path.clone(),
            detail: "this build cannot write the config yet".to_string(),
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
            Err(error) => Err(ConfigError::Io {
                path: self.path.clone(),
                detail: error.to_string(),
            }),
        }
    }

    fn add_repo(&self, _repo: &Repo) -> Result<(), ConfigError> {
        Err(self.writes_unsupported())
    }

    fn remove_repo(&self, _name: &RepoName) -> Result<(), ConfigError> {
        Err(self.writes_unsupported())
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawConfig {
    worktree_root: Option<PathBuf>,
    branch_prefix: Option<String>,
    tmux_prefix: Option<String>,
    state_poll_ms: Option<u64>,
    preview_refresh_ms: Option<u64>,
    diff_refresh_ms: Option<u64>,
    activity_idle_seconds: Option<i64>,
    session_size: Option<String>,
    bell_on_waiting: Option<bool>,
    scan_depth: Option<i64>,
    repos: Vec<RawRepo>,
    agents: Vec<RawAgent>,
}

impl RawConfig {
    fn into_draft(self, defaults: ConfigDraft) -> ConfigDraft {
        let mut draft = defaults;
        if let Some(root) = &self.worktree_root {
            draft = draft.worktree_root(root);
        }
        if let Some(prefix) = &self.branch_prefix {
            draft = draft.branch_prefix(prefix);
        }
        if let Some(prefix) = &self.tmux_prefix {
            draft = draft.tmux_prefix(prefix);
        }
        draft = self.timing_into(draft);
        for repo in self.repos {
            draft = draft.repo(repo.into_draft());
        }
        self.agents
            .into_iter()
            .fold(draft, |draft, agent| draft.agent(agent.into_agent()))
    }

    fn timing_into(&self, mut draft: ConfigDraft) -> ConfigDraft {
        if let Some(value) = self.state_poll_ms {
            draft = draft.state_poll_ms(value);
        }
        if let Some(value) = self.preview_refresh_ms {
            draft = draft.preview_refresh_ms(value);
        }
        if let Some(value) = self.diff_refresh_ms {
            draft = draft.diff_refresh_ms(value);
        }
        if let Some(value) = self.activity_idle_seconds {
            draft = draft.activity_idle_seconds(value);
        }
        if let Some(value) = &self.session_size {
            draft = draft.session_size(value);
        }
        if let Some(value) = self.bell_on_waiting {
            draft = draft.bell_on_waiting(value);
        }
        if let Some(value) = self.scan_depth {
            draft = draft.scan_depth(value);
        }
        draft
    }
}

#[derive(Deserialize)]
struct RawRepo {
    name: Option<String>,
    path: PathBuf,
    base_branch: Option<String>,
}

impl RawRepo {
    fn into_draft(self) -> RepoDraft {
        let draft = RepoDraft::new(&self.path);
        let draft = match &self.name {
            Some(name) => draft.named(name),
            None => draft,
        };
        match &self.base_branch {
            Some(base) => draft.based_on(base),
            None => draft,
        }
    }
}

#[derive(Deserialize)]
struct RawAgent {
    name: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: Vec<String>,
    #[serde(default)]
    prompt_args: Vec<String>,
}

impl RawAgent {
    fn into_agent(self) -> Agent {
        Agent::new(&self.name, &self.command)
            .with_args(&self.args)
            .with_env_names(&self.env)
            .with_prompt_args(&self.prompt_args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../tests/fixtures/config.toml");

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
            TomlConfigSource::new(&self.path(), Path::new("/data/ringleader/worktrees"))
        }

        fn with_text(text: &str) -> Self {
            let fixture = Self::new();
            fs::write(fixture.path(), text).unwrap();
            fixture
        }
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
    fn a_config_without_agents_loads_with_no_agents() {
        let fixture = SourceFixture::with_text("branch_prefix = \"ringleader/\"\n");

        assert!(fixture.source().load().unwrap().agents().is_empty());
    }

    #[test]
    fn an_empty_config_loads() {
        let fixture = SourceFixture::with_text("");

        assert!(fixture.source().load().is_ok());
    }

    #[test]
    fn unparsable_text_is_a_parse_error() {
        let fixture = SourceFixture::with_text("tmux_prefix = ");

        assert!(matches!(
            fixture.source().load(),
            Err(ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn writes_are_refused_until_the_writer_exists() {
        let fixture = SourceFixture::with_text(FIXTURE);
        let repo = Repo::discovered(RepoName::parse("other").unwrap(), Path::new("/repos/other"));

        assert!(fixture.source().add_repo(&repo).is_err());
        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), FIXTURE);
    }
}
