use std::path::{Path, PathBuf};

use super::ports::{AgentLaunch, SessionSpec, StartPoint};
use crate::domain::{
    Agent, BaseBranch, Config, InvalidName, NamingScheme, Registration, Repo, Session,
    SessionMetadata, SessionName, SessionRepo, SessionSize, SessionWorktree, TmuxAddress,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateRequest {
    name: String,
    repo: Repo,
    agent: Option<String>,
    base_branch: Option<BaseBranch>,
    prompt: String,
}

impl CreateRequest {
    pub fn new(name: &str, repo: Repo) -> Self {
        Self {
            name: name.to_string(),
            repo,
            agent: None,
            base_branch: None,
            prompt: String::new(),
        }
    }

    pub fn with_agent(self, agent: &str) -> Self {
        Self {
            agent: Some(agent.to_string()),
            ..self
        }
    }

    pub fn with_base_branch(self, base_branch: BaseBranch) -> Self {
        Self {
            base_branch: Some(base_branch),
            ..self
        }
    }

    pub fn with_prompt(self, prompt: &str) -> Self {
        Self {
            prompt: prompt.to_string(),
            ..self
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    #[cfg(test)]
    pub fn repo(&self) -> &Repo {
        &self.repo
    }

    pub fn agent(&self) -> Option<&str> {
        self.agent.as_deref()
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }
}

pub struct CreationPlan {
    name: SessionName,
    repo: Repo,
    base_branch: Option<BaseBranch>,
    naming: NamingScheme,
    agent: String,
    argv: Vec<String>,
    env_names: Vec<String>,
    env_assignments: Vec<(String, String)>,
    removed_env_names: Vec<String>,
    size: SessionSize,
}

impl CreationPlan {
    pub fn new(
        request: &CreateRequest,
        agent: &Agent,
        config: &Config,
    ) -> Result<Self, InvalidName> {
        Ok(Self {
            name: SessionName::new(&request.name)?,
            repo: request.repo.clone(),
            base_branch: request
                .base_branch
                .clone()
                .or_else(|| request.repo.base_branch().cloned()),
            naming: config.naming().clone(),
            agent: agent.name().to_string(),
            argv: agent.launch_argv(&request.prompt),
            env_names: agent.env_names().to_vec(),
            env_assignments: agent.env_assignments().to_vec(),
            removed_env_names: config.agent_env_union(),
            size: config.session_size(),
        })
    }

    pub fn rooted_at(self, worktree_root: &Path) -> Self {
        let naming = NamingScheme::new(
            worktree_root,
            self.naming.branch_prefix(),
            self.naming.tmux_prefix(),
        );
        Self { naming, ..self }
    }

    pub fn name(&self) -> &SessionName {
        &self.name
    }

    pub fn repo_path(&self) -> &Path {
        self.repo.path()
    }

    pub fn base_branch(&self) -> Option<&BaseBranch> {
        self.base_branch.as_ref()
    }

    pub fn worktree(&self) -> PathBuf {
        self.naming.worktree_path(self.repo.name(), &self.name)
    }

    pub fn branch(&self) -> String {
        self.naming.branch_name(&self.name)
    }

    pub fn tmux_name(&self) -> String {
        self.naming.tmux_session_name(&self.name)
    }

    pub fn spec(&self, start: &StartPoint) -> SessionSpec {
        let launch = AgentLaunch::new(self.argv.clone(), &self.env_names, &self.worktree())
            .with_env_assignments(&self.env_assignments);
        SessionSpec::new(&self.tmux_name(), self.metadata(start), launch)
            .sized(self.size)
            .removing_env_names(&self.removed_env_names)
    }

    pub fn session(&self, start: &StartPoint, socket: &Path, created_at: u64) -> Session {
        let tmux = TmuxAddress::new(&self.tmux_name(), socket);
        let registration = Registration::new(tmux, self.naming.clone(), created_at);
        Session::new(self.name.clone(), self.metadata(start), registration)
    }

    fn metadata(&self, start: &StartPoint) -> SessionMetadata {
        SessionMetadata::new(
            SessionRepo::new(self.repo.name().clone(), self.repo.path()),
            SessionWorktree::new(&self.worktree(), &self.branch(), start.base_commit()),
            &self.agent,
        )
    }
}
