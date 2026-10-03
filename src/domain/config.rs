use std::path::{Path, PathBuf};
use std::time::Duration;

use super::agent::Agent;
use super::repo::Repo;
use super::repo_name::RepoName;
use super::session_name::SessionName;

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
    pub fn defaults(worktree_root: &Path) -> Self {
        Self {
            naming: NamingScheme::new(worktree_root, "ringleader/", "ringleader-"),
            intervals: RefreshIntervals::default(),
            activity_idle_seconds: 3,
            session_size: SessionSize::default(),
            bell_on_waiting: false,
            scan_depth: 3,
            repos: Vec::new(),
            agents: Vec::new(),
        }
    }

    pub fn with_agents(self, agents: Vec<Agent>) -> Self {
        Self { agents, ..self }
    }

    pub fn with_repos(self, repos: Vec<Repo>) -> Self {
        Self { repos, ..self }
    }

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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NamingScheme {
    worktree_root: PathBuf,
    branch_prefix: String,
    tmux_prefix: String,
}

impl NamingScheme {
    pub fn new(worktree_root: &Path, branch_prefix: &str, tmux_prefix: &str) -> Self {
        Self {
            worktree_root: worktree_root.to_path_buf(),
            branch_prefix: branch_prefix.to_string(),
            tmux_prefix: tmux_prefix.to_string(),
        }
    }

    pub fn worktree_root(&self) -> &Path {
        &self.worktree_root
    }

    pub fn branch_prefix(&self) -> &str {
        &self.branch_prefix
    }

    pub fn tmux_prefix(&self) -> &str {
        &self.tmux_prefix
    }

    pub fn tmux_session_name(&self, name: &SessionName) -> String {
        format!("{}{}", self.tmux_prefix, name)
    }

    pub fn branch_name(&self, name: &SessionName) -> String {
        format!("{}{}", self.branch_prefix, name)
    }

    pub fn worktree_path(&self, repo: &RepoName, name: &SessionName) -> PathBuf {
        self.worktree_root.join(repo.as_str()).join(name.as_str())
    }

    pub fn strip_tmux_prefix<'a>(&self, tmux_name: &'a str) -> Option<&'a str> {
        tmux_name.strip_prefix(self.tmux_prefix.as_str())
    }

    pub fn owns_branch(&self, branch: &str) -> bool {
        branch.starts_with(&self.branch_prefix)
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

    struct NamingFixture;

    impl NamingFixture {
        fn default_scheme() -> NamingScheme {
            NamingScheme::new(Path::new("/worktrees"), "ringleader/", "ringleader-")
        }
    }

    #[test]
    fn the_worktree_path_nests_the_session_under_its_repo() {
        let path = NamingFixture::default_scheme().worktree_path(
            &RepoName::parse("example-project").unwrap(),
            &SessionName::new("fix-auth").unwrap(),
        );

        assert_eq!(path, Path::new("/worktrees/example-project/fix-auth"));
    }

    #[test]
    fn strips_the_tmux_prefix() {
        let scheme = NamingFixture::default_scheme();

        assert_eq!(
            scheme.strip_tmux_prefix("ringleader-fix-auth"),
            Some("fix-auth")
        );
    }

    #[test]
    fn a_name_without_the_tmux_prefix_strips_to_nothing() {
        let scheme = NamingFixture::default_scheme();

        assert_eq!(scheme.strip_tmux_prefix("work"), None);
    }

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
