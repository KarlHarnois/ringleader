use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use crate::application::ports::{UsageError, UsageMeter};
use crate::domain::{Agent, AgentUsage};

#[derive(Default)]
pub struct FakeUsageMeter {
    state: Mutex<FakeState>,
}

#[derive(Default)]
struct FakeState {
    usages: HashMap<String, AgentUsage>,
    failing_agents: HashSet<String>,
    calls: Vec<String>,
}

impl FakeUsageMeter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_usage(self, agent: &str, usage: AgentUsage) -> Self {
        self.state
            .lock()
            .unwrap()
            .usages
            .insert(agent.to_string(), usage);
        self
    }

    pub fn fail(&self, agent: &str) {
        self.state
            .lock()
            .unwrap()
            .failing_agents
            .insert(agent.to_string());
    }

    pub fn recover(&self, agent: &str) {
        self.state.lock().unwrap().failing_agents.remove(agent);
    }

    pub fn calls(&self) -> Vec<String> {
        self.state.lock().unwrap().calls.clone()
    }
}

impl UsageMeter for FakeUsageMeter {
    fn measure(&self, agent: &Agent) -> Result<Option<AgentUsage>, UsageError> {
        let mut state = self.state.lock().unwrap();
        state.calls.push(format!("measure {}", agent.name()));
        if state.failing_agents.contains(agent.name()) {
            return Err(UsageError::Unanswered);
        }
        Ok(state.usages.get(agent.name()).cloned())
    }
}
