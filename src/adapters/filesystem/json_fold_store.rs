use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::durable_file::DurableFile;
use crate::application::ports::{FoldStore, FoldStoreError};
use crate::domain::{RepoGroup, RepoName};

pub struct JsonFoldStore {
    file: DurableFile,
}

impl JsonFoldStore {
    pub fn new(path: &Path) -> Self {
        Self {
            file: DurableFile::new(path),
        }
    }

    fn path(&self) -> &Path {
        self.file.path()
    }

    fn corrupt(&self, detail: impl ToString) -> FoldStoreError {
        FoldStoreError::Corrupt {
            path: self.path().to_path_buf(),
            detail: detail.to_string(),
        }
    }

    fn io_error(&self, error: io::Error) -> FoldStoreError {
        FoldStoreError::Io {
            path: self.path().to_path_buf(),
            detail: error.to_string(),
        }
    }

    fn parse(&self, text: &str) -> Result<HashSet<RepoGroup>, FoldStoreError> {
        let document: FoldDocument =
            serde_json::from_str(text).map_err(|error| self.corrupt(error))?;
        document
            .folded
            .into_iter()
            .map(|entry| {
                entry
                    .into_group()
                    .ok_or_else(|| self.corrupt("an entry has an invalid repo name"))
            })
            .collect()
    }
}

impl FoldStore for JsonFoldStore {
    fn load(&self) -> Result<HashSet<RepoGroup>, FoldStoreError> {
        match fs::read_to_string(self.path()) {
            Ok(text) => self.parse(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(HashSet::new()),
            Err(error) => Err(self.io_error(error)),
        }
    }

    fn save(&self, folded: &HashSet<RepoGroup>) -> Result<(), FoldStoreError> {
        let document = FoldDocument::in_repo_order(folded);
        let text = serde_json::to_string_pretty(&document).map_err(|error| self.corrupt(error))?;
        self.file
            .write(text.as_bytes())
            .map_err(|error| self.io_error(error))
    }
}

#[derive(Serialize, Deserialize)]
struct FoldDocument {
    folded: Vec<FoldEntry>,
}

impl FoldDocument {
    fn in_repo_order(folded: &HashSet<RepoGroup>) -> Self {
        let ordered: BTreeSet<&RepoGroup> = folded.iter().collect();
        Self {
            folded: ordered.into_iter().map(FoldEntry::from_group).collect(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ServiceFixture;

    struct FoldStoreFixture {
        directory: tempfile::TempDir,
    }

    impl FoldStoreFixture {
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
            self.directory.path().join("folds.json")
        }

        fn store(&self) -> JsonFoldStore {
            JsonFoldStore::new(&self.path())
        }

        fn saved_json(&self) -> serde_json::Value {
            serde_json::from_str(&fs::read_to_string(self.path()).unwrap()).unwrap()
        }

        fn folded(groups: &[RepoGroup]) -> HashSet<RepoGroup> {
            groups.iter().cloned().collect()
        }
    }

    #[test]
    fn a_missing_file_holds_no_folds() {
        let fixture = FoldStoreFixture::new();

        assert!(fixture.store().load().unwrap().is_empty());
    }

    #[test]
    fn saved_folds_load_back_unchanged() {
        let fixture = FoldStoreFixture::new();
        let folded = FoldStoreFixture::folded(&[
            ServiceFixture::repo_group("/repos/alpha"),
            RepoGroup::Other,
        ]);

        fixture.store().save(&folded).unwrap();

        assert_eq!(fixture.store().load().unwrap(), folded);
    }

    #[test]
    fn a_save_writes_each_fold_by_kind_in_repo_order_so_equal_folds_write_equal_files() {
        let fixture = FoldStoreFixture::new();
        let folded = FoldStoreFixture::folded(&[
            RepoGroup::Other,
            ServiceFixture::repo_group("/repos/beta"),
            ServiceFixture::repo_group("/repos/alpha"),
        ]);

        fixture.store().save(&folded).unwrap();

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
    fn unparsable_text_is_corrupt() {
        let fixture = FoldStoreFixture::with_contents("{ not json");

        let error = fixture.store().load().unwrap_err();

        assert!(matches!(error, FoldStoreError::Corrupt { .. }));
    }

    #[test]
    fn an_entry_with_an_invalid_repo_name_is_corrupt() {
        let fixture = FoldStoreFixture::with_contents(
            r#"{ "folded": [ { "kind": "repo", "name": "a/b", "path": "/repos/a" } ] }"#,
        );

        let error = fixture.store().load().unwrap_err();

        assert!(matches!(error, FoldStoreError::Corrupt { .. }));
    }

    #[test]
    fn a_save_into_a_missing_directory_is_an_io_error() {
        let fixture = FoldStoreFixture::new();
        let store = JsonFoldStore::new(&fixture.directory.path().join("missing/folds.json"));

        let error = store.save(&HashSet::from([RepoGroup::Other])).unwrap_err();

        assert!(matches!(error, FoldStoreError::Io { .. }));
    }
}
