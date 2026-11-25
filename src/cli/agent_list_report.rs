use serde::Serialize;

use super::Reply;
use crate::domain::{Agent, AgentMode};

pub struct AgentListReport<'a> {
    agents: &'a [Agent],
}

impl<'a> AgentListReport<'a> {
    const PLAN_SUFFIX: &'static str = "  plan";

    pub fn new(agents: &'a [Agent]) -> Self {
        Self { agents }
    }

    fn line(entry: &AgentListEntry) -> String {
        let suffix = if entry.plan { Self::PLAN_SUFFIX } else { "" };
        format!("{:<24} {}{suffix}", entry.name, entry.command)
    }
}

impl Reply for AgentListReport<'_> {
    fn lines(&self) -> Vec<String> {
        self.agents
            .iter()
            .map(|agent| Self::line(&AgentListEntry::of(agent)))
            .collect()
    }

    fn result(&self) -> impl Serialize {
        AgentListResult {
            agents: self.agents.iter().map(AgentListEntry::of).collect(),
        }
    }
}

#[derive(Serialize)]
struct AgentListResult {
    agents: Vec<AgentListEntry>,
}

#[derive(Serialize)]
struct AgentListEntry {
    name: String,
    command: String,
    plan: bool,
}

impl AgentListEntry {
    fn of(agent: &Agent) -> Self {
        Self {
            name: agent.name().to_string(),
            command: agent.command().to_string(),
            plan: agent.supports(AgentMode::Plan),
        }
    }
}
