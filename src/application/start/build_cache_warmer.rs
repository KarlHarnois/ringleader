use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::creation_plan::CreationPlan;
use crate::application::ServicePorts;
use crate::application::ports::{BuildCache, Workspace};
use crate::domain::BuildCacheDir;

pub struct BuildCacheWarmer {
    workspace: Arc<dyn Workspace>,
    build_cache: Arc<dyn BuildCache>,
}

impl BuildCacheWarmer {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            workspace: ports.repos().workspace(),
            build_cache: ports.repos().build_cache(),
        }
    }

    pub fn warm(&self, plan: &CreationPlan) {
        let dirs = plan.build_cache_dirs();
        if dirs.is_empty() {
            return;
        }
        let siblings = self.sibling_worktrees(plan.repo_path());
        for dir in dirs {
            self.warm_dir(dir, &siblings, &plan.worktree());
        }
    }

    fn sibling_worktrees(&self, repo: &Path) -> Vec<PathBuf> {
        match self.workspace.list_worktrees(repo) {
            Ok(entries) => entries
                .iter()
                .map(|entry| entry.path().to_path_buf())
                .collect(),
            Err(error) => {
                log::warn!("cannot list worktrees to warm the build cache: {error}");
                Vec::new()
            }
        }
    }

    fn warm_dir(&self, dir: &BuildCacheDir, siblings: &[PathBuf], worktree: &Path) {
        let Some(source) = self.most_recent_build(dir, siblings) else {
            return;
        };
        if let Err(error) = self.build_cache.copy(&source, &dir.within(worktree)) {
            log::warn!("{error}");
        }
    }

    fn most_recent_build(&self, dir: &BuildCacheDir, siblings: &[PathBuf]) -> Option<PathBuf> {
        siblings
            .iter()
            .map(|sibling| dir.within(sibling))
            .filter_map(|candidate| Some((self.build_cache.last_built(&candidate)?, candidate)))
            .max_by_key(|(built, _)| *built)
            .map(|(_, candidate)| candidate)
    }
}
