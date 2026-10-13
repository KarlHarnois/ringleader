use std::path::{Path, PathBuf};

use crate::domain::Agent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLaunch {
    argv: Vec<String>,
    env_names: Vec<String>,
    env_assignments: Vec<(String, String)>,
    worktree: PathBuf,
}

impl AgentLaunch {
    pub fn new(argv: Vec<String>, env_names: &[String], worktree: &Path) -> Self {
        Self {
            argv,
            env_names: env_names.to_vec(),
            env_assignments: Vec::new(),
            worktree: worktree.to_path_buf(),
        }
    }

    pub fn for_agent(agent: &Agent, argv: Vec<String>, worktree: &Path) -> Self {
        Self::new(argv, agent.env_names(), worktree).with_env_assignments(agent.env_assignments())
    }

    pub fn with_env_assignments(self, assignments: &[(String, String)]) -> Self {
        Self {
            env_assignments: assignments.to_vec(),
            ..self
        }
    }

    pub fn argv(&self) -> &[String] {
        &self.argv
    }

    pub fn env_names(&self) -> &[String] {
        &self.env_names
    }

    pub fn env_assignments(&self) -> &[(String, String)] {
        &self.env_assignments
    }

    pub fn worktree(&self) -> &Path {
        &self.worktree
    }
}
