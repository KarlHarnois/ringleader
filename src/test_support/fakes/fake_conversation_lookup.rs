use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::application::ports::{ConversationLookup, ConversationLookupError};
use crate::domain::Agent;

#[derive(Default)]
pub struct FakeConversationLookup {
    state: Mutex<FakeState>,
}

#[derive(Default)]
struct FakeState {
    answer: Option<Result<String, ConversationLookupError>>,
    worktrees: Vec<PathBuf>,
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

    pub fn worktrees(&self) -> Vec<PathBuf> {
        self.state.lock().unwrap().worktrees.clone()
    }
}

impl ConversationLookup for FakeConversationLookup {
    fn find(
        &self,
        _agent: &Agent,
        worktree: &Path,
    ) -> Result<Option<String>, ConversationLookupError> {
        let mut state = self.state.lock().unwrap();
        state.worktrees.push(worktree.to_path_buf());
        state.answer.clone().transpose()
    }
}
