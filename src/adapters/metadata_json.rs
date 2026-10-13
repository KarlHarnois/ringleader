use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::domain::{RepoName, SessionMetadata, SessionRepo, SessionWorktree};

#[derive(Serialize, Deserialize)]
pub struct MetadataJson {
    repo_name: String,
    repo_path: PathBuf,
    branch: String,
    base_commit: String,
    worktree_path: PathBuf,
    agent: String,
}

impl MetadataJson {
    pub fn encode(metadata: &SessionMetadata) -> String {
        let record = Self {
            repo_name: metadata.repo_name().to_string(),
            repo_path: metadata.repo_path().to_path_buf(),
            branch: metadata.branch().to_string(),
            base_commit: metadata.base_commit().to_string(),
            worktree_path: metadata.worktree_path().to_path_buf(),
            agent: metadata.agent().to_string(),
        };
        serde_json::to_string(&record).unwrap_or_default()
    }

    pub fn decode(text: &str) -> Option<SessionMetadata> {
        let record: Self = serde_json::from_str(text).ok()?;
        let repo = SessionRepo::new(RepoName::parse(&record.repo_name).ok()?, &record.repo_path);
        let worktree =
            SessionWorktree::new(&record.worktree_path, &record.branch, &record.base_commit);
        Some(SessionMetadata::new(repo, worktree, &record.agent))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn metadata_survives_an_encode_decode_round_trip() {
        let metadata = SessionMetadata::new(
            SessionRepo::new(
                RepoName::parse("example-project").unwrap(),
                Path::new("/repos/example-project"),
            ),
            SessionWorktree::new(
                Path::new("/worktrees/example-project/fix-auth"),
                "ringleader/fix-auth",
                "feedface",
            ),
            "claude",
        );

        let decoded = MetadataJson::decode(&MetadataJson::encode(&metadata)).unwrap();

        assert_eq!(decoded, metadata);
    }

    #[test]
    fn text_that_is_not_metadata_decodes_to_nothing() {
        assert_eq!(MetadataJson::decode("{\"repo_name\": 1}"), None);
    }
}
