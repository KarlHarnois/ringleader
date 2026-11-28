use std::sync::Mutex;

use super::opencode_database::{OpencodeDatabase, OpencodeDatabaseError};
use crate::domain::Agent;

pub struct FakeOpencodeDatabase {
    answer: Result<Option<String>, OpencodeDatabaseError>,
    queries: Mutex<Vec<String>>,
}

impl FakeOpencodeDatabase {
    pub fn answering(answer: Result<Option<String>, OpencodeDatabaseError>) -> Self {
        Self {
            answer,
            queries: Mutex::default(),
        }
    }

    pub fn queries(&self) -> Vec<String> {
        self.queries.lock().unwrap().clone()
    }
}

impl OpencodeDatabase for FakeOpencodeDatabase {
    fn read(&self, _agent: &Agent, query: &str) -> Result<Option<String>, OpencodeDatabaseError> {
        self.queries.lock().unwrap().push(query.to_string());
        self.answer.clone()
    }
}
