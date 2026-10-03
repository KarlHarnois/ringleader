use std::path::Path;

use super::ports::HostProbe;
use super::service_error::ServiceError;
use crate::domain::{Agent, Config};

pub struct UseChecks<'a> {
    host: &'a dyn HostProbe,
    config: &'a Config,
    config_path: &'a Path,
}

impl<'a> UseChecks<'a> {
    pub fn new(host: &'a dyn HostProbe, config: &'a Config, config_path: &'a Path) -> Self {
        Self {
            host,
            config,
            config_path,
        }
    }

    pub fn agent_for_creation(
        &self,
        requested: Option<&str>,
        prompt: &str,
    ) -> Result<&'a Agent, ServiceError> {
        let agent = self.requested_agent(requested)?;
        self.check_command(agent)?;
        self.check_worktree_root()?;
        match agent.prompt_refusal(prompt) {
            Some(refusal) => Err(ServiceError::Invalid(refusal)),
            None => Ok(agent),
        }
    }

    pub fn agent_for_restart(&self, name: &str) -> Result<&'a Agent, ServiceError> {
        let agent = self.config.agent(name).ok_or_else(|| {
            ServiceError::Invalid(format!(
                "agent {name} is no longer configured in {}, run ringleader doctor",
                self.config_path.display()
            ))
        })?;
        self.check_command(agent)?;
        Ok(agent)
    }

    fn requested_agent(&self, requested: Option<&str>) -> Result<&'a Agent, ServiceError> {
        let config_path = self.config_path.display();
        match requested {
            Some(name) => self.config.agent(name).ok_or_else(|| {
                ServiceError::Invalid(format!("agent {name} is not configured in {config_path}"))
            }),
            None => self.config.default_agent().ok_or_else(|| {
                ServiceError::Invalid(format!("no agents configured in {config_path}"))
            }),
        }
    }

    fn check_command(&self, agent: &Agent) -> Result<(), ServiceError> {
        if self.host.resolve_command(agent.command()).is_some() {
            return Ok(());
        }
        Err(ServiceError::Invalid(
            UnresolvedCommand::new(agent).message(),
        ))
    }

    fn check_worktree_root(&self) -> Result<(), ServiceError> {
        let root = self.config.naming().worktree_root();
        let nearest_existing = root.ancestors().find(|path| self.host.path_exists(path));
        if nearest_existing.is_some_and(|directory| self.host.is_writable(directory)) {
            return Ok(());
        }
        Err(ServiceError::Invalid(format!(
            "worktree root {} is not writable",
            root.display()
        )))
    }
}

pub struct UnresolvedCommand<'a> {
    agent: &'a Agent,
}

impl<'a> UnresolvedCommand<'a> {
    pub fn new(agent: &'a Agent) -> Self {
        Self { agent }
    }

    pub fn message(&self) -> String {
        let command = self.agent.command();
        let hint = if command.contains(char::is_whitespace) {
            ", move the flags into args"
        } else {
            ""
        };
        format!(
            "agent {} command `{command}` was not found{hint}",
            self.agent.name()
        )
    }
}
