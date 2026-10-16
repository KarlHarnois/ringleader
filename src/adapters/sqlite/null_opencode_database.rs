use crate::adapters::opencode::opencode_database::OpencodeDatabase;
use crate::domain::Agent;

pub struct NullOpencodeDatabase;

impl OpencodeDatabase for NullOpencodeDatabase {
    fn read(&self, _agent: &Agent, _query: &str) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_query_can_be_read() {
        let agent = Agent::new("opencode", "opencode");

        assert_eq!(NullOpencodeDatabase.read(&agent, "select 1"), None);
    }
}
