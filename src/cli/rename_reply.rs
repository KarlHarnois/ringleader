use serde::Serialize;

use super::Reply;
use crate::domain::Session;

pub struct RenameReply {
    old: String,
    renamed: Session,
}

impl RenameReply {
    pub fn new(old: &str, renamed: Session) -> Self {
        Self {
            old: old.to_string(),
            renamed,
        }
    }
}

impl Reply for RenameReply {
    fn lines(&self) -> Vec<String> {
        vec![format!("renamed {} to {}", self.old, self.renamed.name())]
    }

    fn result(&self) -> impl Serialize {
        RenameResult {
            old: self.old.clone(),
            new: self.renamed.name().to_string(),
        }
    }
}

#[derive(Serialize)]
struct RenameResult {
    old: String,
    new: String,
}
