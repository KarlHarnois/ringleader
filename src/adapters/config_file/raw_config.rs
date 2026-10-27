use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::domain::{Agent, ConfigDraft, RepoDraft};

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct RawConfig {
    worktree_root: Option<PathBuf>,
    return_key: Option<String>,
    next_session_keys: Option<Vec<String>>,
    previous_session_keys: Option<Vec<String>>,
    shell_toggle_keys: Option<Vec<String>>,
    finder_keys: Option<Vec<String>>,
    menu_keys: Option<Vec<String>>,
    state_poll_ms: Option<u64>,
    preview_refresh_ms: Option<u64>,
    diff_refresh_ms: Option<u64>,
    pull_request_refresh_ms: Option<u64>,
    activity_idle_seconds: Option<i64>,
    session_size: Option<String>,
    bell_on_waiting: Option<bool>,
    scan_depth: Option<i64>,
    repos: Vec<RawRepo>,
    agents: Vec<RawAgent>,
}

impl RawConfig {
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|error| ParseFailure::new(text, &error).describe())
    }

    pub fn into_draft(self, defaults: ConfigDraft, home: Option<&Path>) -> ConfigDraft {
        let home = HomeExpansion::new(home);
        self.expanded_draft(defaults, &home)
    }

    fn expanded_draft(self, defaults: ConfigDraft, home: &HomeExpansion) -> ConfigDraft {
        let mut draft =
            self.keys_into(self.session_options_into(self.refresh_intervals_into(defaults)));
        if let Some(root) = &self.worktree_root {
            draft = draft.worktree_root(&home.path(root));
        }
        for repo in self.repos {
            draft = draft.repo(repo.into_draft(home));
        }
        self.agents
            .into_iter()
            .fold(draft, |draft, agent| draft.agent(agent.into_agent(home)))
    }

    fn keys_into(&self, mut draft: ConfigDraft) -> ConfigDraft {
        if let Some(key) = &self.return_key {
            draft = draft.return_key(key);
        }
        if let Some(keys) = &self.next_session_keys {
            draft = draft.next_session_keys(keys);
        }
        if let Some(keys) = &self.previous_session_keys {
            draft = draft.previous_session_keys(keys);
        }
        if let Some(keys) = &self.shell_toggle_keys {
            draft = draft.shell_toggle_keys(keys);
        }
        if let Some(keys) = &self.finder_keys {
            draft = draft.finder_keys(keys);
        }
        if let Some(keys) = &self.menu_keys {
            draft = draft.menu_keys(keys);
        }
        draft
    }

    fn refresh_intervals_into(&self, mut draft: ConfigDraft) -> ConfigDraft {
        if let Some(value) = self.state_poll_ms {
            draft = draft.state_poll_ms(value);
        }
        if let Some(value) = self.preview_refresh_ms {
            draft = draft.preview_refresh_ms(value);
        }
        if let Some(value) = self.diff_refresh_ms {
            draft = draft.diff_refresh_ms(value);
        }
        if let Some(value) = self.pull_request_refresh_ms {
            draft = draft.pull_request_refresh_ms(value);
        }
        draft
    }

    fn session_options_into(&self, mut draft: ConfigDraft) -> ConfigDraft {
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
    fn into_draft(self, home: &HomeExpansion) -> RepoDraft {
        let draft = RepoDraft::new(&home.path(&self.path));
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
    set_env: BTreeMap<String, String>,
    #[serde(default)]
    prompt_args: Vec<String>,
    #[serde(default)]
    plan_args: Vec<String>,
    #[serde(default)]
    resume_args: Vec<String>,
    #[serde(default)]
    startup_input: Vec<String>,
    #[serde(default)]
    rename_input: Vec<String>,
    #[serde(default = "RawAgent::built_in_skills_by_default")]
    built_in_skills: bool,
}

impl RawAgent {
    fn built_in_skills_by_default() -> bool {
        true
    }

    fn into_agent(self, home: &HomeExpansion) -> Agent {
        let agent = self.configured_agent(home);
        match self.built_in_skills {
            true => agent,
            false => agent.without_built_in_skills(),
        }
    }

    fn configured_agent(&self, home: &HomeExpansion) -> Agent {
        Agent::new(&self.name, &home.command(&self.command))
            .with_args(&self.args)
            .with_env_names(&self.env)
            .with_env_assignments(&self.expanded_assignments(home))
            .with_prompt_args(&self.prompt_args)
            .with_plan_args(&self.plan_args)
            .with_resume_args(&self.resume_args)
            .with_startup_input(&self.startup_input)
            .with_rename_input(&self.rename_input)
    }

    fn expanded_assignments(&self, home: &HomeExpansion) -> Vec<(String, String)> {
        self.set_env
            .iter()
            .map(|(name, value)| (name.clone(), home.value(value)))
            .collect()
    }
}

struct HomeExpansion<'a> {
    home: Option<&'a Path>,
}

impl<'a> HomeExpansion<'a> {
    fn new(home: Option<&'a Path>) -> Self {
        Self { home }
    }

    fn path(&self, path: &Path) -> PathBuf {
        let text = path.to_string_lossy();
        match (self.home, text.as_ref()) {
            (Some(home), "~") => home.to_path_buf(),
            (Some(home), _) => match text.strip_prefix("~/") {
                Some(rest) => home.join(rest),
                None => path.to_path_buf(),
            },
            (None, _) => path.to_path_buf(),
        }
    }

    fn value(&self, value: &str) -> String {
        self.path(Path::new(value)).to_string_lossy().into_owned()
    }

    fn command(&self, command: &str) -> String {
        match (self.home, command.strip_prefix("~/")) {
            (Some(home), Some(rest)) => home.join(rest).to_string_lossy().into_owned(),
            _ => command.to_string(),
        }
    }
}

struct ParseFailure<'a> {
    text: &'a str,
    error: &'a toml::de::Error,
}

impl<'a> ParseFailure<'a> {
    fn new(text: &'a str, error: &'a toml::de::Error) -> Self {
        Self { text, error }
    }

    fn describe(&self) -> String {
        let message = self.error.message().trim();
        match self.offending_line() {
            Some(line) => format!("`{}`: {message}", line.trim()),
            None => message.to_string(),
        }
    }

    fn offending_line(&self) -> Option<&'a str> {
        let start = self.error.span()?.start.min(self.text.len());
        let line_start = self.text[..start].rfind('\n').map_or(0, |index| index + 1);
        self.text[line_start..].lines().next()
    }
}
