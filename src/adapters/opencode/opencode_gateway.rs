use std::sync::Arc;

use super::opencode_database::OpencodeDatabase;
use crate::adapters::agent_launch::agent_probe::AgentProbe;
use crate::application::ports::UsageError;
use crate::domain::Agent;

pub struct OpencodeGateway {
    probe: Arc<AgentProbe>,
    database: Arc<dyn OpencodeDatabase>,
}

impl OpencodeGateway {
    const PROGRAM: &'static str = "opencode";
    const DATABASE_ARGUMENTS: [&'static str; 4] = ["--pure", "db", "--format", "json"];

    pub fn new(probe: Arc<AgentProbe>, database: Arc<dyn OpencodeDatabase>) -> Self {
        Self { probe, database }
    }

    pub fn serves(agent: &Agent) -> bool {
        agent.command_name() == Self::PROGRAM
    }

    pub fn query(&self, agent: &Agent, query: &str) -> Result<String, UsageError> {
        let direct = self.database.read(agent, query).unwrap_or_else(|error| {
            log::info!("{error}, asking opencode instead");
            None
        });
        match direct {
            Some(output) => Ok(output),
            None => self.ask_opencode(agent, query),
        }
    }

    fn ask_opencode(&self, agent: &Agent, query: &str) -> Result<String, UsageError> {
        let mut arguments = Self::DATABASE_ARGUMENTS.to_vec();
        arguments.push(query);
        self.probe.run(agent, &arguments)
    }
}
