use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use crate::application::ports::{BuildCache, BuildCacheError};

#[derive(Default)]
pub struct FakeBuildCache {
    state: Mutex<BuildCacheState>,
}

#[derive(Default)]
struct BuildCacheState {
    built: BTreeMap<PathBuf, SystemTime>,
    copies: Vec<(PathBuf, PathBuf)>,
    failing: bool,
}

impl FakeBuildCache {
    pub fn built_at(self, directory: &Path, seconds_after_epoch: u64) -> Self {
        let built = SystemTime::UNIX_EPOCH + Duration::from_secs(seconds_after_epoch);
        self.state
            .lock()
            .unwrap()
            .built
            .insert(directory.to_path_buf(), built);
        self
    }

    pub fn failing(self) -> Self {
        self.state.lock().unwrap().failing = true;
        self
    }

    pub fn copies(&self) -> Vec<(PathBuf, PathBuf)> {
        self.state.lock().unwrap().copies.clone()
    }
}

impl BuildCache for FakeBuildCache {
    fn last_built(&self, directory: &Path) -> Option<SystemTime> {
        self.state.lock().unwrap().built.get(directory).copied()
    }

    fn copy(&self, source: &Path, destination: &Path) -> Result<(), BuildCacheError> {
        let mut state = self.state.lock().unwrap();
        if state.failing {
            return Err(BuildCacheError::Copy {
                origin: source.to_path_buf(),
                destination: destination.to_path_buf(),
                detail: "copy-on-write clones are not supported".to_string(),
            });
        }
        state
            .copies
            .push((source.to_path_buf(), destination.to_path_buf()));
        Ok(())
    }
}
