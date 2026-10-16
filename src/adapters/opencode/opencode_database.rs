use crate::domain::Agent;

pub trait OpencodeDatabase: Send + Sync {
    fn read(&self, agent: &Agent, query: &str) -> Option<String>;
}
