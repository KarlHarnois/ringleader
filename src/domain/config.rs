use std::path::Path;
use std::time::Duration;

use super::agent::Agent;
use super::config_draft::ConfigDraft;
use super::naming_scheme::NamingScheme;
use super::repo::Repo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    naming: NamingScheme,
    intervals: RefreshIntervals,
    activity_idle_seconds: u64,
    session_size: SessionSize,
    bell_on_waiting: bool,
    scan_depth: usize,
    repos: Vec<Repo>,
    agents: Vec<Agent>,
}

impl Config {
    pub fn new(draft: &ConfigDraft) -> Result<Self, Vec<String>> {
        let failures = draft.failures();
        if !failures.is_empty() {
            return Err(failures);
        }
        Ok(Self {
            naming: draft.naming(),
            intervals: draft.intervals(),
            activity_idle_seconds: draft.activity_idle_seconds_value(),
            session_size: draft.session_size_value(),
            bell_on_waiting: draft.bell_on_waiting_value(),
            scan_depth: draft.scan_depth_value(),
            repos: draft.repos_value(),
            agents: draft.agents_value().to_vec(),
        })
    }

    pub fn defaults(worktree_root: &Path) -> Self {
        Self {
            naming: NamingScheme::new(worktree_root, "ringleader-"),
            intervals: RefreshIntervals::default(),
            activity_idle_seconds: 3,
            session_size: SessionSize::default(),
            bell_on_waiting: false,
            scan_depth: 3,
            repos: Vec::new(),
            agents: Vec::new(),
        }
    }

    #[cfg(test)]
    pub fn with_agents(self, agents: Vec<Agent>) -> Self {
        Self { agents, ..self }
    }

    #[cfg(test)]
    pub fn with_repos(self, repos: Vec<Repo>) -> Self {
        Self { repos, ..self }
    }

    #[cfg(test)]
    pub fn with_naming(self, naming: NamingScheme) -> Self {
        Self { naming, ..self }
    }

    pub fn naming(&self) -> &NamingScheme {
        &self.naming
    }

    pub fn intervals(&self) -> RefreshIntervals {
        self.intervals
    }

    pub fn activity_idle_seconds(&self) -> u64 {
        self.activity_idle_seconds
    }

    pub fn session_size(&self) -> SessionSize {
        self.session_size
    }

    pub fn bell_on_waiting(&self) -> bool {
        self.bell_on_waiting
    }

    pub fn scan_depth(&self) -> usize {
        self.scan_depth
    }

    pub fn repos(&self) -> &[Repo] {
        &self.repos
    }

    pub fn agents(&self) -> &[Agent] {
        &self.agents
    }

    pub fn agent(&self, name: &str) -> Option<&Agent> {
        self.agents.iter().find(|agent| agent.name() == name)
    }

    pub fn default_agent(&self) -> Option<&Agent> {
        self.agents.first()
    }

    pub fn agent_env_union(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for name in self.agents.iter().flat_map(|agent| agent.env_names()) {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
        names
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionSize {
    width: u16,
    height: u16,
}

impl SessionSize {
    pub fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let (width, height) = text.split_once('x')?;
        let all_digits = |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit());
        if !all_digits(width) || !all_digits(height) {
            return None;
        }
        Some(Self::new(width.parse().ok()?, height.parse().ok()?))
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }
}

impl Default for SessionSize {
    fn default() -> Self {
        Self::new(200, 50)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshIntervals {
    state_poll: Duration,
    preview_refresh: Duration,
    diff_refresh: Duration,
}

impl RefreshIntervals {
    pub fn new(state_poll: Duration, preview_refresh: Duration, diff_refresh: Duration) -> Self {
        Self {
            state_poll,
            preview_refresh,
            diff_refresh,
        }
    }

    pub fn state_poll(&self) -> Duration {
        self.state_poll
    }

    pub fn preview_refresh(&self) -> Duration {
        self.preview_refresh
    }

    pub fn diff_refresh(&self) -> Duration {
        self.diff_refresh
    }
}

impl Default for RefreshIntervals {
    fn default() -> Self {
        Self::new(
            Duration::from_millis(1000),
            Duration::from_millis(750),
            Duration::from_millis(3000),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_env_union_lists_each_name_once_in_config_order() {
        let first = Agent::new("a", "a").with_env_names(&["KEY_A".into(), "SHARED".into()]);
        let second = Agent::new("b", "b").with_env_names(&["SHARED".into(), "KEY_B".into()]);

        let config = Config::defaults(Path::new("/worktrees")).with_agents(vec![first, second]);

        assert_eq!(config.agent_env_union(), ["KEY_A", "SHARED", "KEY_B"]);
    }

    #[test]
    fn the_first_agent_is_the_default() {
        let agents = vec![
            Agent::new("claude", "claude"),
            Agent::new("opencode", "opencode"),
        ];

        let config = Config::defaults(Path::new("/worktrees")).with_agents(agents);

        assert_eq!(config.default_agent().unwrap().name(), "claude");
    }
}
