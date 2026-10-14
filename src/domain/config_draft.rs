use std::iter::once;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::agent::Agent;
use super::config::{RefreshIntervals, SessionSize};
use super::config_checks::ConfigChecks;
use super::hot_key::HotKey;
use super::key_bindings::KeyBindings;
use super::naming_scheme::NamingScheme;
use super::repo::Repo;
use super::repo_draft::RepoDraft;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigDraft {
    worktree_root: PathBuf,
    return_key: String,
    next_session_keys: Option<Vec<String>>,
    previous_session_keys: Option<Vec<String>>,
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
            return_key: KeyBindings::RETURN_KEY.to_string(),
            next_session_keys: None,
            previous_session_keys: None,
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

    pub fn next_session_keys(self, keys: &[impl AsRef<str>]) -> Self {
        Self {
            next_session_keys: Some(Self::owned(keys)),
            ..self
        }
    }

    pub fn previous_session_keys(self, keys: &[impl AsRef<str>]) -> Self {
        Self {
            previous_session_keys: Some(Self::owned(keys)),
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

    pub(super) fn raw_next_session_keys(&self) -> Vec<String> {
        self.written_or_default(
            self.next_session_keys.as_deref(),
            &KeyBindings::NEXT_SESSION_KEYS,
        )
    }

    pub(super) fn raw_previous_session_keys(&self) -> Vec<String> {
        self.written_or_default(
            self.previous_session_keys.as_deref(),
            &KeyBindings::PREVIOUS_SESSION_KEYS,
        )
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

    pub(super) fn return_key_value(&self) -> HotKey {
        HotKey::parse(&self.return_key)
            .unwrap_or_else(|| KeyBindings::default().return_key().clone())
    }

    pub(super) fn next_session_keys_value(&self) -> Vec<HotKey> {
        HotKey::parse_valid(self.raw_next_session_keys())
    }

    pub(super) fn previous_session_keys_value(&self) -> Vec<HotKey> {
        HotKey::parse_valid(self.raw_previous_session_keys())
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

    fn written_or_default(&self, written: Option<&[String]>, defaults: &[&str]) -> Vec<String> {
        match written {
            Some(keys) => keys.to_vec(),
            None => self.defaults_not_written_elsewhere(defaults),
        }
    }

    fn defaults_not_written_elsewhere(&self, defaults: &[&str]) -> Vec<String> {
        let written = self.written_keys();
        defaults
            .iter()
            .filter(|key| HotKey::parse(key).is_some_and(|key| !written.contains(&key)))
            .map(|key| key.to_string())
            .collect()
    }

    fn written_keys(&self) -> Vec<HotKey> {
        let lists = [&self.next_session_keys, &self.previous_session_keys]
            .into_iter()
            .flatten()
            .flatten();
        HotKey::parse_valid(once(&self.return_key).chain(lists))
    }

    fn owned(keys: &[impl AsRef<str>]) -> Vec<String> {
        keys.iter().map(|key| key.as_ref().to_string()).collect()
    }
}
