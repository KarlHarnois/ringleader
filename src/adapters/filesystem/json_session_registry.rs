use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::durable_file::DurableFile;
use crate::application::ports::{Clock, RegistryError, SessionRegistry};
use crate::domain::{
    BaseBranch, NamingScheme, Registration, RepoName, Session, SessionMetadata, SessionName,
    SessionRepo, SessionWorktree, TmuxAddress,
};

pub struct JsonSessionRegistry {
    file: DurableFile,
    clock: Arc<dyn Clock>,
}

impl JsonSessionRegistry {
    pub const SCHEMA_VERSION: u64 = 3;

    pub fn new(path: &Path, clock: Arc<dyn Clock>) -> Self {
        Self {
            file: DurableFile::new(path),
            clock,
        }
    }

    fn path(&self) -> &Path {
        self.file.path()
    }

    fn corrupt(&self, detail: impl ToString) -> RegistryError {
        RegistryError::Corrupt {
            path: self.path().to_path_buf(),
            detail: detail.to_string(),
        }
    }

    fn io_error(&self, error: io::Error) -> RegistryError {
        RegistryError::Io {
            path: self.path().to_path_buf(),
            detail: error.to_string(),
        }
    }

    fn parse(&self, text: &str) -> Result<Vec<Session>, RegistryError> {
        let header: VersionHeader =
            serde_json::from_str(text).map_err(|error| self.corrupt(error))?;
        if header.version > Self::SCHEMA_VERSION {
            return Err(RegistryError::NewerSchema {
                found: header.version,
                supported: Self::SCHEMA_VERSION,
            });
        }
        let document: RegistryDocument =
            serde_json::from_str(text).map_err(|error| self.corrupt(error))?;
        document
            .sessions
            .into_iter()
            .map(|entry| {
                entry
                    .into_session()
                    .ok_or_else(|| self.corrupt("an entry has an invalid name"))
            })
            .collect()
    }
}

impl SessionRegistry for JsonSessionRegistry {
    fn load(&self) -> Result<Vec<Session>, RegistryError> {
        match fs::read_to_string(self.path()) {
            Ok(text) => self.parse(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(self.io_error(error)),
        }
    }

    fn save(&self, sessions: &[Session]) -> Result<(), RegistryError> {
        let document = RegistryDocument {
            version: Self::SCHEMA_VERSION,
            sessions: sessions.iter().map(SessionEntry::from_session).collect(),
        };
        let text = serde_json::to_string_pretty(&document).map_err(|error| self.corrupt(error))?;
        self.file
            .write(text.as_bytes())
            .map_err(|error| self.io_error(error))
    }

    fn move_aside_corrupt(&self) -> Result<PathBuf, RegistryError> {
        let name = format!("sessions.json.corrupt-{}", self.clock.now_epoch_seconds());
        let destination = self.path().with_file_name(name);
        fs::rename(self.path(), &destination).map_err(|error| self.io_error(error))?;
        Ok(destination)
    }
}

#[derive(Deserialize)]
struct VersionHeader {
    version: u64,
}

#[derive(Serialize, Deserialize)]
struct RegistryDocument {
    version: u64,
    sessions: Vec<SessionEntry>,
}

#[derive(Serialize, Deserialize)]
struct SessionEntry {
    name: String,
    repo_name: String,
    repo_path: PathBuf,
    worktree_path: PathBuf,
    branch: String,
    base_commit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    base_branch: Option<String>,
    agent: String,
    tmux_session_name: String,
    socket_path: PathBuf,
    created_at: u64,
    worktree_root: PathBuf,
    tmux_prefix: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    favorite: Option<bool>,
}

impl SessionEntry {
    fn from_session(session: &Session) -> Self {
        let naming = session.naming();
        Self {
            name: session.name().to_string(),
            repo_name: session.repo_name().to_string(),
            repo_path: session.repo_path().to_path_buf(),
            worktree_path: session.worktree_path().to_path_buf(),
            branch: session.branch().to_string(),
            base_commit: session.base_commit().to_string(),
            base_branch: session.requested_base_branch().map(ToString::to_string),
            agent: session.agent().to_string(),
            tmux_session_name: session.tmux_name().to_string(),
            socket_path: session.socket_path().to_path_buf(),
            created_at: session.created_at(),
            worktree_root: naming.worktree_root().to_path_buf(),
            tmux_prefix: naming.tmux_prefix().to_string(),
            favorite: session.is_favorite().then_some(true),
        }
    }

    fn into_session(self) -> Option<Session> {
        let metadata = SessionMetadata::new(
            SessionRepo::new(RepoName::parse(&self.repo_name).ok()?, &self.repo_path),
            SessionWorktree::new(&self.worktree_path, &self.branch, &self.base_commit)
                .based_on(self.base_branch.as_deref().and_then(BaseBranch::new)),
            &self.agent,
        );
        let registration = Registration::new(
            TmuxAddress::new(&self.tmux_session_name, &self.socket_path),
            NamingScheme::new(&self.worktree_root, &self.tmux_prefix),
            self.created_at,
        );
        let favorite = self.favorite.unwrap_or(false);
        Some(
            Session::new(
                SessionName::from_slug(&self.name).ok()?,
                metadata,
                registration,
            )
            .with_favorite(favorite),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{FakeClock, ServiceFixture};

    const FIXTURE: &str = include_str!("../../../tests/fixtures/sessions.json");
    const VERSION_ONE: &str = include_str!("../../../tests/fixtures/sessions-v1.json");

    struct RegistryFixture {
        directory: tempfile::TempDir,
    }

    impl RegistryFixture {
        const NOW: u64 = 1_791_000_500;

        fn new() -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
            }
        }

        fn with_contents(text: &str) -> Self {
            let fixture = Self::new();
            fs::write(fixture.path(), text).unwrap();
            fixture
        }

        fn path(&self) -> PathBuf {
            self.directory.path().join("sessions.json")
        }

        fn registry(&self) -> JsonSessionRegistry {
            JsonSessionRegistry::new(&self.path(), Arc::new(FakeClock::at(Self::NOW)))
        }
    }

    #[test]
    fn a_missing_file_is_an_empty_registry() {
        let fixture = RegistryFixture::new();

        assert!(fixture.registry().load().unwrap().is_empty());
    }

    #[test]
    fn the_fixture_registry_loads_every_entry() {
        let fixture = RegistryFixture::with_contents(FIXTURE);

        let sessions = fixture.registry().load().unwrap();

        let names: Vec<&str> = sessions
            .iter()
            .map(|session| session.name().as_str())
            .collect();
        assert_eq!(names, ["fix-auth", "dark-mode"]);
        assert_eq!(sessions[0].naming().tmux_prefix(), "ringleader-");
        assert!(sessions[0].is_favorite());
        assert!(!sessions[1].is_favorite());
    }

    #[test]
    fn a_requested_base_branch_survives_a_save_and_load() {
        let fixture = RegistryFixture::new();
        let session = ServiceFixture::session_based_on("fix-auth", "develop");

        fixture.registry().save(&[session]).unwrap();

        let loaded = fixture.registry().load().unwrap();
        assert_eq!(
            loaded[0].requested_base_branch(),
            BaseBranch::new("develop").as_ref()
        );
    }

    #[test]
    fn a_session_without_a_requested_base_branch_writes_no_field_for_it() {
        let fixture = RegistryFixture::new();

        fixture
            .registry()
            .save(&[ServiceFixture::session("fix-auth")])
            .unwrap();

        assert!(
            !fs::read_to_string(fixture.path())
                .unwrap()
                .contains("base_branch")
        );
    }

    #[test]
    fn an_entry_written_without_a_base_branch_loads_without_one() {
        let fixture = RegistryFixture::with_contents(FIXTURE);

        let sessions = fixture.registry().load().unwrap();

        assert_eq!(sessions[0].requested_base_branch(), None);
    }

    #[test]
    fn an_entry_written_without_a_favorite_loads_as_not_a_favorite() {
        let fixture = RegistryFixture::with_contents(VERSION_ONE);

        let sessions = fixture.registry().load().unwrap();

        assert!(!sessions[0].is_favorite());
    }

    #[test]
    fn a_favorite_survives_a_save_and_load() {
        let fixture = RegistryFixture::new();
        let session = ServiceFixture::session("fix-auth").with_favorite(true);

        fixture.registry().save(&[session]).unwrap();

        assert!(fixture.registry().load().unwrap()[0].is_favorite());
    }

    #[test]
    fn a_session_that_is_not_a_favorite_writes_no_field_for_it() {
        let fixture = RegistryFixture::new();

        fixture
            .registry()
            .save(&[ServiceFixture::session("fix-auth")])
            .unwrap();

        assert!(
            !fs::read_to_string(fixture.path())
                .unwrap()
                .contains("favorite")
        );
    }

    #[test]
    fn a_version_one_registry_loads_its_prefixed_branches() {
        let fixture = RegistryFixture::with_contents(VERSION_ONE);

        let sessions = fixture.registry().load().unwrap();

        assert_eq!(sessions[0].branch(), "ringleader/fix-auth");
    }

    #[test]
    fn saving_a_loaded_version_one_registry_writes_the_current_version() {
        let fixture = RegistryFixture::with_contents(VERSION_ONE);
        let registry = fixture.registry();
        let sessions = registry.load().unwrap();

        registry.save(&sessions).unwrap();

        let text = fs::read_to_string(fixture.path()).unwrap();
        let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(saved["version"], 3);
        assert!(saved["sessions"][0].get("branch_prefix").is_none());
    }

    #[test]
    fn a_saved_registry_loads_back_unchanged() {
        let fixture = RegistryFixture::new();
        let sessions = vec![
            ServiceFixture::session("fix-auth"),
            ServiceFixture::session("docs"),
        ];

        fixture.registry().save(&sessions).unwrap();

        assert_eq!(fixture.registry().load().unwrap(), sessions);
    }

    #[test]
    fn a_save_leaves_only_the_registry_file() {
        let fixture = RegistryFixture::new();

        fixture
            .registry()
            .save(&[ServiceFixture::session("fix-auth")])
            .unwrap();

        assert_eq!(fs::read_dir(fixture.directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn unparsable_text_is_corrupt_and_left_in_place() {
        let fixture = RegistryFixture::with_contents("{ not json");

        let error = fixture.registry().load().unwrap_err();

        assert!(matches!(error, RegistryError::Corrupt { .. }));
        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), "{ not json");
    }

    #[test]
    fn a_corrupt_registry_is_moved_aside_with_a_timestamp() {
        let fixture = RegistryFixture::with_contents("{ not json");

        let moved = fixture.registry().move_aside_corrupt().unwrap();

        assert_eq!(
            moved,
            fixture
                .directory
                .path()
                .join("sessions.json.corrupt-1791000500")
        );
        assert!(!fixture.path().exists());
    }

    #[test]
    fn a_newer_schema_with_another_shape_is_newer_rather_than_corrupt() {
        let newer = r#"{"version": 9, "sessions": {"renamed": "shape"}}"#;
        let fixture = RegistryFixture::with_contents(newer);

        let error = fixture.registry().load().unwrap_err();

        assert_eq!(
            error,
            RegistryError::NewerSchema {
                found: 9,
                supported: 3
            }
        );
        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), newer);
    }

    #[test]
    fn an_entry_with_an_invalid_name_is_corrupt() {
        let invalid = FIXTURE.replace("\"fix-auth\"", "\"Fix Auth\"");
        let fixture = RegistryFixture::with_contents(&invalid);

        assert!(matches!(
            fixture.registry().load(),
            Err(RegistryError::Corrupt { .. })
        ));
    }
}
