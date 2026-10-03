use std::path::{Path, PathBuf};
use std::time::Duration;

use super::agent::Agent;
use super::base_branch::BaseBranch;
use super::config::{RefreshIntervals, SessionSize};
use super::config_checks::ConfigChecks;
use super::naming_scheme::NamingScheme;
use super::repo::Repo;
use super::repo_name::RepoName;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigDraft {
    worktree_root: PathBuf,
    branch_prefix: String,
    tmux_prefix: String,
    state_poll_ms: u64,
    preview_refresh_ms: u64,
    diff_refresh_ms: u64,
    activity_idle_seconds: i64,
    session_size: String,
    bell_on_waiting: bool,
    scan_depth: i64,
    repos: Vec<RepoDraft>,
    agents: Vec<Agent>,
}

impl ConfigDraft {
    pub fn new(default_worktree_root: &Path) -> Self {
        Self {
            worktree_root: default_worktree_root.to_path_buf(),
            branch_prefix: "ringleader/".to_string(),
            tmux_prefix: "ringleader-".to_string(),
            state_poll_ms: 1000,
            preview_refresh_ms: 750,
            diff_refresh_ms: 3000,
            activity_idle_seconds: 3,
            session_size: "200x50".to_string(),
            bell_on_waiting: false,
            scan_depth: 3,
            repos: Vec::new(),
            agents: Vec::new(),
        }
    }

    pub fn worktree_root(self, worktree_root: &Path) -> Self {
        Self {
            worktree_root: worktree_root.to_path_buf(),
            ..self
        }
    }

    pub fn branch_prefix(self, branch_prefix: &str) -> Self {
        Self {
            branch_prefix: branch_prefix.to_string(),
            ..self
        }
    }

    pub fn tmux_prefix(self, tmux_prefix: &str) -> Self {
        Self {
            tmux_prefix: tmux_prefix.to_string(),
            ..self
        }
    }

    pub fn state_poll_ms(self, state_poll_ms: u64) -> Self {
        Self {
            state_poll_ms,
            ..self
        }
    }

    pub fn preview_refresh_ms(self, preview_refresh_ms: u64) -> Self {
        Self {
            preview_refresh_ms,
            ..self
        }
    }

    pub fn diff_refresh_ms(self, diff_refresh_ms: u64) -> Self {
        Self {
            diff_refresh_ms,
            ..self
        }
    }

    pub fn activity_idle_seconds(self, activity_idle_seconds: i64) -> Self {
        Self {
            activity_idle_seconds,
            ..self
        }
    }

    pub fn session_size(self, session_size: &str) -> Self {
        Self {
            session_size: session_size.to_string(),
            ..self
        }
    }

    pub fn bell_on_waiting(self, bell_on_waiting: bool) -> Self {
        Self {
            bell_on_waiting,
            ..self
        }
    }

    pub fn scan_depth(self, scan_depth: i64) -> Self {
        Self { scan_depth, ..self }
    }

    pub fn repo(mut self, repo: RepoDraft) -> Self {
        self.repos.push(repo);
        self
    }

    pub fn agent(mut self, agent: Agent) -> Self {
        self.agents.push(agent);
        self
    }

    pub fn failures(&self) -> Vec<String> {
        ConfigChecks::new(self).failures()
    }

    pub(super) fn raw_worktree_root(&self) -> &Path {
        &self.worktree_root
    }

    pub(super) fn raw_branch_prefix(&self) -> &str {
        &self.branch_prefix
    }

    pub(super) fn raw_tmux_prefix(&self) -> &str {
        &self.tmux_prefix
    }

    pub(super) fn raw_activity_idle_seconds(&self) -> i64 {
        self.activity_idle_seconds
    }

    pub(super) fn raw_session_size(&self) -> &str {
        &self.session_size
    }

    pub(super) fn parsed_session_size(&self) -> Option<SessionSize> {
        SizeText::new(&self.session_size).parsed()
    }

    pub(super) fn raw_scan_depth(&self) -> i64 {
        self.scan_depth
    }

    pub(super) fn repo_drafts(&self) -> &[RepoDraft] {
        &self.repos
    }

    pub(super) fn naming(&self) -> NamingScheme {
        NamingScheme::new(&self.worktree_root, &self.branch_prefix, &self.tmux_prefix)
    }

    pub(super) fn intervals(&self) -> RefreshIntervals {
        RefreshIntervals::new(
            Duration::from_millis(self.state_poll_ms),
            Duration::from_millis(self.preview_refresh_ms),
            Duration::from_millis(self.diff_refresh_ms),
        )
    }

    pub(super) fn activity_idle_seconds_value(&self) -> u64 {
        u64::try_from(self.activity_idle_seconds).unwrap_or_default()
    }

    pub(super) fn session_size_value(&self) -> SessionSize {
        SizeText::new(&self.session_size)
            .parsed()
            .unwrap_or_default()
    }

    pub(super) fn bell_on_waiting_value(&self) -> bool {
        self.bell_on_waiting
    }

    pub(super) fn scan_depth_value(&self) -> usize {
        usize::try_from(self.scan_depth).unwrap_or_default()
    }

    pub(super) fn repos_value(&self) -> Vec<Repo> {
        self.repos.iter().filter_map(RepoDraft::repo).collect()
    }

    pub(super) fn agents_value(&self) -> &[Agent] {
        &self.agents
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoDraft {
    name: Option<String>,
    path: PathBuf,
    base_branch: Option<String>,
}

impl RepoDraft {
    pub fn new(path: &Path) -> Self {
        Self {
            name: None,
            path: path.to_path_buf(),
            base_branch: None,
        }
    }

    pub fn named(self, name: &str) -> Self {
        Self {
            name: Some(name.to_string()),
            ..self
        }
    }

    pub fn based_on(self, base_branch: &str) -> Self {
        Self {
            base_branch: Some(base_branch.to_string()),
            ..self
        }
    }

    fn repo_name(&self) -> Option<RepoName> {
        match &self.name {
            Some(name) => RepoName::parse(name).ok(),
            None => RepoName::from_basename(&self.path.file_name()?.to_string_lossy()).ok(),
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn shown_name(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| self.path.display().to_string())
    }

    pub(super) fn has_valid_name(&self) -> bool {
        self.repo_name().is_some()
    }

    pub(super) fn resolved_name(&self) -> Option<RepoName> {
        self.repo_name()
    }

    fn repo(&self) -> Option<Repo> {
        let base_branch = self.base_branch.as_deref().and_then(BaseBranch::new);
        Some(Repo::explicit(self.repo_name()?, &self.path, base_branch))
    }
}

struct SizeText<'a> {
    text: &'a str,
}

impl<'a> SizeText<'a> {
    fn new(text: &'a str) -> Self {
        Self { text }
    }

    fn parsed(&self) -> Option<SessionSize> {
        let (width, height) = self.text.split_once('x')?;
        let all_digits = |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit());
        if !all_digits(width) || !all_digits(height) {
            return None;
        }
        Some(SessionSize::new(width.parse().ok()?, height.parse().ok()?))
    }
}
