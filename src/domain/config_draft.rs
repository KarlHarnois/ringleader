use std::path::{Path, PathBuf};
use std::time::Duration;

use super::agent::Agent;
use super::config::{RefreshIntervals, SessionSize};
use super::config_checks::ConfigChecks;
use super::naming_scheme::NamingScheme;
use super::repo::Repo;
use super::repo_draft::RepoDraft;
use super::return_key::ReturnKey;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigDraft {
    worktree_root: PathBuf,
    return_key: String,
    state_poll_ms: u64,
    preview_refresh_ms: u64,
    diff_refresh_ms: u64,
    pull_request_refresh_ms: u64,
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
            return_key: ReturnKey::DEFAULT.to_string(),
            state_poll_ms: 1000,
            preview_refresh_ms: 750,
            diff_refresh_ms: 3000,
            pull_request_refresh_ms: 30000,
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

    pub fn return_key(self, return_key: &str) -> Self {
        Self {
            return_key: return_key.to_string(),
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

    pub fn pull_request_refresh_ms(self, pull_request_refresh_ms: u64) -> Self {
        Self {
            pull_request_refresh_ms,
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

    pub(super) fn raw_return_key(&self) -> &str {
        &self.return_key
    }

    pub(super) fn raw_activity_idle_seconds(&self) -> i64 {
        self.activity_idle_seconds
    }

    pub(super) fn raw_session_size(&self) -> &str {
        &self.session_size
    }

    pub(super) fn parsed_session_size(&self) -> Option<SessionSize> {
        SessionSize::parse(&self.session_size)
    }

    pub(super) fn raw_scan_depth(&self) -> i64 {
        self.scan_depth
    }

    pub(super) fn repo_drafts(&self) -> &[RepoDraft] {
        &self.repos
    }

    pub(super) fn naming(&self) -> NamingScheme {
        NamingScheme::for_new_sessions(&self.worktree_root)
    }

    pub(super) fn intervals(&self) -> RefreshIntervals {
        RefreshIntervals::new(
            Duration::from_millis(self.state_poll_ms),
            Duration::from_millis(self.preview_refresh_ms),
            Duration::from_millis(self.diff_refresh_ms),
            Duration::from_millis(self.pull_request_refresh_ms),
        )
    }

    pub(super) fn return_key_value(&self) -> ReturnKey {
        ReturnKey::parse(&self.return_key).unwrap_or_default()
    }

    pub(super) fn activity_idle_seconds_value(&self) -> u64 {
        u64::try_from(self.activity_idle_seconds).unwrap_or_default()
    }

    pub(super) fn session_size_value(&self) -> SessionSize {
        SessionSize::parse(&self.session_size).unwrap_or_default()
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
