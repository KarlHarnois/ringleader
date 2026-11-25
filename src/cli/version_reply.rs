use serde::Serialize;

use super::reply::Reply;

#[derive(Serialize)]
struct VersionResult {}

pub struct VersionReply {
    line: String,
}

impl VersionReply {
    pub(super) fn new(line: String) -> Self {
        Self { line }
    }
}

impl Reply for VersionReply {
    fn lines(&self) -> Vec<String> {
        vec![self.line.clone()]
    }

    fn result(&self) -> impl Serialize {
        VersionResult {}
    }
}
