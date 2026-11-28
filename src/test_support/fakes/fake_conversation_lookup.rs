use std::sync::Mutex;

use crate::application::ports::{ConversationLookup, ConversationLookupError, ConversationQuery};

#[derive(Default)]
pub struct FakeConversationLookup {
    state: Mutex<FakeState>,
}

#[derive(Default)]
struct FakeState {
    answer: Option<Result<String, ConversationLookupError>>,
    queries: Vec<ConversationQuery>,
}

impl FakeConversationLookup {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn finding(self, conversation: &str) -> Self {
        self.answering(Ok(conversation.to_string()))
    }

    pub fn failing(self, error: ConversationLookupError) -> Self {
        self.answering(Err(error))
    }

    fn answering(self, answer: Result<String, ConversationLookupError>) -> Self {
        self.state.lock().unwrap().answer = Some(answer);
        self
    }

    pub fn queries(&self) -> Vec<ConversationQuery> {
        self.state.lock().unwrap().queries.clone()
    }
}

impl ConversationLookup for FakeConversationLookup {
    fn find(&self, query: &ConversationQuery) -> Result<Option<String>, ConversationLookupError> {
        let mut state = self.state.lock().unwrap();
        state.queries.push(query.clone());
        state.answer.clone().transpose()
    }
}
