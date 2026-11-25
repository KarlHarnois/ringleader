use serde::Serialize;

use super::{Remark, Reply};
use crate::application::StartOutcome;

pub struct CreateReply {
    outcome: StartOutcome,
}

impl CreateReply {
    pub fn new(outcome: StartOutcome) -> Self {
        Self { outcome }
    }
}

impl Reply for CreateReply {
    fn lines(&self) -> Vec<String> {
        let session = self.outcome.session();
        vec![format!(
            "created {} in {} on branch {}",
            session.name(),
            session.worktree_path().display(),
            session.branch()
        )]
    }

    fn remarks(&self) -> Vec<Remark> {
        self.outcome
            .warning()
            .map(|warning| Remark::Note(warning.to_string()))
            .into_iter()
            .collect()
    }

    fn result(&self) -> impl Serialize {
        CreateResult {
            session: CreatedSession::of(&self.outcome),
            warning: self.outcome.warning().map(str::to_string),
        }
    }
}

#[derive(Serialize)]
struct CreateResult {
    session: CreatedSession,
    warning: Option<String>,
}

#[derive(Serialize)]
struct CreatedSession {
    name: String,
    repo: String,
    branch: String,
    worktree: String,
    agent: String,
}

impl CreatedSession {
    fn of(outcome: &StartOutcome) -> Self {
        let session = outcome.session();
        Self {
            name: session.name().to_string(),
            repo: session.repo_name().to_string(),
            branch: session.branch().to_string(),
            worktree: session.worktree_path().display().to_string(),
            agent: session.agent().to_string(),
        }
    }
}
