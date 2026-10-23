use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::application::ports::{ViewState, ViewStore, ViewStoreError};
use crate::domain::{RepoGroup, RepoName, SessionFilter};

pub struct JsonViewStore {
    path: PathBuf,
}

impl JsonViewStore {
    const PRIVATE_MODE: u32 = 0o600;

    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    fn write_without_syncing(&self, text: &str) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(Self::PRIVATE_MODE)
            .open(&self.path)?;
        file.write_all(text.as_bytes())
    }

    fn corrupt(&self, detail: impl ToString) -> ViewStoreError {
        ViewStoreError::Corrupt {
            path: self.path.clone(),
            detail: detail.to_string(),
        }
    }

    fn io_error(&self, error: io::Error) -> ViewStoreError {
        ViewStoreError::Io {
            path: self.path.clone(),
            detail: error.to_string(),
        }
    }

    fn parse(&self, text: &str) -> Result<ViewState, ViewStoreError> {
        let document: ViewDocument =
            serde_json::from_str(text).map_err(|error| self.corrupt(error))?;
        let folded = document
            .folded
            .into_iter()
            .map(|entry| {
                entry
                    .into_group()
                    .ok_or_else(|| self.corrupt("an entry has an invalid repo name"))
            })
            .collect::<Result<_, _>>()?;
        Ok(ViewState::new(folded, document.filter.into_filter()))
    }
}

impl ViewStore for JsonViewStore {
    fn load(&self) -> Result<ViewState, ViewStoreError> {
        match fs::read_to_string(&self.path) {
            Ok(text) => self.parse(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(ViewState::default()),
            Err(error) => Err(self.io_error(error)),
        }
    }

    fn save(&self, state: &ViewState) -> Result<(), ViewStoreError> {
        let document = ViewDocument::from_state(state);
        let text = serde_json::to_string_pretty(&document).map_err(|error| self.corrupt(error))?;
        self.write_without_syncing(&text)
            .map_err(|error| self.io_error(error))
    }
}

#[derive(Serialize, Deserialize)]
struct ViewDocument {
    #[serde(default)]
    folded: Vec<FoldEntry>,
    #[serde(default, skip_serializing_if = "FilterEntry::is_all")]
    filter: FilterEntry,
}

impl ViewDocument {
    fn from_state(state: &ViewState) -> Self {
        let ordered: BTreeSet<&RepoGroup> = state.folded().iter().collect();
        Self {
            folded: ordered.into_iter().map(FoldEntry::from_group).collect(),
            filter: FilterEntry::from_filter(state.filter()),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum FoldEntry {
    Repo { name: String, path: PathBuf },
    Other,
}

impl FoldEntry {
    fn from_group(group: &RepoGroup) -> Self {
        match group {
            RepoGroup::Named(name, path) => Self::Repo {
                name: name.to_string(),
                path: path.clone(),
            },
            RepoGroup::Other => Self::Other,
        }
    }

    fn into_group(self) -> Option<RepoGroup> {
        match self {
            Self::Repo { name, path } => Some(RepoGroup::Named(RepoName::parse(&name).ok()?, path)),
            Self::Other => Some(RepoGroup::Other),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum FilterEntry {
    #[default]
    All,
    Favorites,
}

impl FilterEntry {
    fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }

    fn from_filter(filter: SessionFilter) -> Self {
        match filter {
            SessionFilter::All => Self::All,
            SessionFilter::Favorites => Self::Favorites,
        }
    }

    fn into_filter(self) -> SessionFilter {
        match self {
            Self::All => SessionFilter::All,
            Self::Favorites => SessionFilter::Favorites,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::test_support::ServiceFixture;

    struct ViewStoreFixture {
        directory: tempfile::TempDir,
    }

    impl ViewStoreFixture {
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
            self.directory.path().join("view.json")
        }

        fn store(&self) -> JsonViewStore {
            JsonViewStore::new(&self.path())
        }

        fn saved_json(&self) -> serde_json::Value {
            serde_json::from_str(&fs::read_to_string(self.path()).unwrap()).unwrap()
        }

        fn folded(groups: &[RepoGroup]) -> HashSet<RepoGroup> {
            groups.iter().cloned().collect()
        }

        fn state(groups: &[RepoGroup], filter: SessionFilter) -> ViewState {
            ViewState::new(Self::folded(groups), filter)
        }
    }

    #[test]
    fn a_missing_file_holds_no_folds_and_shows_every_session() {
        let fixture = ViewStoreFixture::new();

        let state = fixture.store().load().unwrap();

        assert!(state.folded().is_empty());
        assert_eq!(state.filter(), SessionFilter::All);
    }

    #[test]
    fn saved_view_state_loads_back_unchanged() {
        let fixture = ViewStoreFixture::new();
        let state = ViewStoreFixture::state(
            &[ServiceFixture::repo_group("/repos/alpha"), RepoGroup::Other],
            SessionFilter::Favorites,
        );

        fixture.store().save(&state).unwrap();

        assert_eq!(fixture.store().load().unwrap(), state);
    }

    #[test]
    fn a_save_writes_each_fold_by_kind_in_repo_order_so_equal_folds_write_equal_files() {
        let fixture = ViewStoreFixture::new();
        let state = ViewStoreFixture::state(
            &[
                RepoGroup::Other,
                ServiceFixture::repo_group("/repos/beta"),
                ServiceFixture::repo_group("/repos/alpha"),
            ],
            SessionFilter::All,
        );

        fixture.store().save(&state).unwrap();

        assert_eq!(
            fixture.saved_json(),
            serde_json::json!({
                "folded": [
                    { "kind": "repo", "name": "alpha", "path": "/repos/alpha" },
                    { "kind": "repo", "name": "beta", "path": "/repos/beta" },
                    { "kind": "other" },
                ]
            })
        );
    }

    #[test]
    fn a_favorites_filter_is_written_and_loads_back() {
        let fixture = ViewStoreFixture::new();
        let state = ViewStoreFixture::state(&[], SessionFilter::Favorites);

        fixture.store().save(&state).unwrap();

        assert_eq!(fixture.saved_json()["filter"], "favorites");
        assert_eq!(
            fixture.store().load().unwrap().filter(),
            SessionFilter::Favorites
        );
    }

    #[test]
    fn a_document_without_a_filter_shows_every_session() {
        let fixture = ViewStoreFixture::with_contents(r#"{ "folded": [] }"#);

        assert_eq!(fixture.store().load().unwrap().filter(), SessionFilter::All);
    }

    #[test]
    fn a_saved_view_file_is_readable_only_by_its_owner() {
        let fixture = ViewStoreFixture::new();

        fixture.store().save(&ViewState::default()).unwrap();

        let mode = fs::metadata(fixture.path()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn an_unreadable_file_is_an_io_error_so_its_failure_is_logged() {
        let fixture = ViewStoreFixture::new();
        fs::create_dir(fixture.path()).unwrap();

        let error = fixture.store().load().unwrap_err();

        assert!(matches!(error, ViewStoreError::Io { .. }));
    }

    #[test]
    fn unparsable_text_is_corrupt() {
        let fixture = ViewStoreFixture::with_contents("{ not json");

        let error = fixture.store().load().unwrap_err();

        assert!(matches!(error, ViewStoreError::Corrupt { .. }));
    }

    #[test]
    fn an_unknown_filter_is_corrupt() {
        let fixture = ViewStoreFixture::with_contents(r#"{ "filter": "some" }"#);

        let error = fixture.store().load().unwrap_err();

        assert!(matches!(error, ViewStoreError::Corrupt { .. }));
    }

    #[test]
    fn an_entry_with_an_invalid_repo_name_is_corrupt() {
        let fixture = ViewStoreFixture::with_contents(
            r#"{ "folded": [ { "kind": "repo", "name": "a/b", "path": "/repos/a" } ] }"#,
        );

        let error = fixture.store().load().unwrap_err();

        assert!(matches!(error, ViewStoreError::Corrupt { .. }));
    }

    #[test]
    fn a_save_into_a_missing_directory_is_an_io_error() {
        let fixture = ViewStoreFixture::new();
        let store = JsonViewStore::new(&fixture.directory.path().join("missing/view.json"));

        let error = store.save(&ViewState::default()).unwrap_err();

        assert!(matches!(error, ViewStoreError::Io { .. }));
    }
}
