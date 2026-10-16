use super::ports::UsageError;
use crate::domain::AgentUsage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageReading {
    measured_at: u64,
    agents: Vec<AgentReading>,
}

impl UsageReading {
    pub fn new(measured_at: u64, agents: Vec<AgentReading>) -> Self {
        Self {
            measured_at,
            agents,
        }
    }

    pub fn measured_at(&self) -> u64 {
        self.measured_at
    }

    pub fn agents(&self) -> &[AgentReading] {
        &self.agents
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentReading {
    agent: String,
    usage: Result<AgentUsage, UsageError>,
}

impl AgentReading {
    pub fn new(agent: &str, usage: Result<AgentUsage, UsageError>) -> Self {
        Self {
            agent: agent.to_string(),
            usage,
        }
    }

    pub fn agent(&self) -> &str {
        &self.agent
    }

    pub fn usage(&self) -> Result<&AgentUsage, &UsageError> {
        self.usage.as_ref()
    }
}
