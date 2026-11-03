use std::fs;
use std::iter::once;
use std::path::Path;
use std::time::SystemTime;

use crate::adapters::TreeCloner;
use crate::application::ports::{BuildCache, BuildCacheError};

pub struct CopyOnWriteBuildCache {
    tree: TreeCloner,
}

impl CopyOnWriteBuildCache {
    pub fn new() -> Self {
        Self {
            tree: TreeCloner::new(),
        }
    }

    fn modified(path: &Path) -> Option<SystemTime> {
        fs::symlink_metadata(path).ok()?.modified().ok()
    }

    fn newest_child(directory: &Path) -> Option<SystemTime> {
        fs::read_dir(directory)
            .ok()?
            .flatten()
            .filter_map(|entry| Self::modified(&entry.path()))
            .max()
    }

    fn create_parent(destination: &Path) -> Result<(), String> {
        match destination.parent() {
            Some(parent) => fs::create_dir_all(parent).map_err(|error| error.to_string()),
            None => Ok(()),
        }
    }
}

impl BuildCache for CopyOnWriteBuildCache {
    fn last_built(&self, directory: &Path) -> Option<SystemTime> {
        let own = Self::modified(directory)?;
        Self::newest_child(directory)
            .into_iter()
            .chain(once(own))
            .max()
    }

    fn copy(&self, source: &Path, destination: &Path) -> Result<(), BuildCacheError> {
        if fs::symlink_metadata(destination).is_ok() {
            return Err(BuildCacheError::Occupied {
                destination: destination.to_path_buf(),
            });
        }
        Self::create_parent(destination)
            .and_then(|()| self.tree.clone_tree(source, destination))
            .map_err(|detail| BuildCacheError::Copy {
                origin: source.to_path_buf(),
                destination: destination.to_path_buf(),
                detail,
            })
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;

    struct CacheFixture {
        directory: tempfile::TempDir,
    }

    impl CacheFixture {
        fn new() -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
            }
        }

        fn cache() -> CopyOnWriteBuildCache {
            CopyOnWriteBuildCache::new()
        }

        fn path(&self, relative: &str) -> PathBuf {
            self.directory.path().join(relative)
        }

        fn directory_built_at(&self, relative: &str, seconds_after_epoch: u64) -> PathBuf {
            let path = self.path(relative);
            fs::create_dir_all(&path).unwrap();
            Self::touch(&path, seconds_after_epoch);
            path
        }

        fn touch(path: &Path, seconds_after_epoch: u64) {
            let time = SystemTime::UNIX_EPOCH + Duration::from_secs(seconds_after_epoch);
            File::open(path).unwrap().set_modified(time).unwrap();
        }
    }

    #[test]
    fn a_missing_directory_was_never_built() {
        let fixture = CacheFixture::new();

        assert_eq!(
            CacheFixture::cache().last_built(&fixture.path("target")),
            None
        );
    }

    #[test]
    fn a_build_is_dated_by_its_newest_profile_directory() {
        let fixture = CacheFixture::new();
        let target = fixture.directory_built_at("target", 100);
        fixture.directory_built_at("target/debug", 300);
        CacheFixture::touch(&target, 100);

        let built = CacheFixture::cache().last_built(&target);

        assert_eq!(
            built,
            Some(SystemTime::UNIX_EPOCH + Duration::from_secs(300))
        );
    }

    #[test]
    fn an_existing_destination_is_kept_instead_of_copied_into() {
        let fixture = CacheFixture::new();
        let source = fixture.directory_built_at("source/target", 100);
        let destination = fixture.directory_built_at("new/target", 100);

        let result = CacheFixture::cache().copy(&source, &destination);

        assert_eq!(result, Err(BuildCacheError::Occupied { destination }));
    }

    #[test]
    fn a_missing_source_is_reported_as_a_failed_copy() {
        let fixture = CacheFixture::new();

        let result =
            CacheFixture::cache().copy(&fixture.path("absent"), &fixture.path("new/target"));

        assert!(
            matches!(result, Err(BuildCacheError::Copy { .. })),
            "{result:?}"
        );
    }
}
