use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use crate::application::ports::{UsageError, UsageMeter};
use crate::domain::{Agent, AgentUsage};
use crate::test_support::CallHooks;

#[derive(Default)]
pub struct FakeUsageMeter {
    state: Mutex<FakeState>,
    hooks: CallHooks,
}

#[derive(Default)]
struct FakeState {
    usages: HashMap<String, AgentUsage>,
    failing_agents: HashSet<String>,
    calls: Vec<String>,
}

impl FakeUsageMeter {
    const MEASURE: &'static str = "measure";

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

    pub fn on_measure(&self, hook: impl Fn() + Send + Sync + 'static) {
        self.hooks.add(Self::MEASURE, hook);
    }

    pub fn calls(&self) -> Vec<String> {
        self.state.lock().unwrap().calls.clone()
    }
}

impl UsageMeter for FakeUsageMeter {
    fn measure(&self, agent: &Agent) -> Result<Option<AgentUsage>, UsageError> {
        self.state
            .lock()
            .unwrap()
            .calls
            .push(format!("{} {}", Self::MEASURE, agent.name()));
        self.hooks.run(Self::MEASURE);
        let state = self.state.lock().unwrap();
        if state.failing_agents.contains(agent.name()) {
            return Err(UsageError::Unanswered);
        }
        Ok(state.usages.get(agent.name()).cloned())
    }
}
