use std::path::{Path, PathBuf};

use thiserror::Error;

pub trait ScratchSpace: Send + Sync {
    fn directory(&self, worktree: &Path) -> PathBuf;
    fn create(&self, worktree: &Path) -> Result<(), ScratchSpaceError>;
    fn remove(&self, worktree: &Path) -> Result<(), ScratchSpaceError>;
    fn directories(&self) -> Result<Vec<PathBuf>, ScratchSpaceError>;
    fn retire(&self, directory: &Path) -> Result<(), ScratchSpaceError>;
    fn retired_directories(&self) -> Result<Vec<PathBuf>, ScratchSpaceError>;
    fn purge(&self, retired: &Path) -> Result<(), ScratchSpaceError>;
    fn tool_temp_directories(
        &self,
        directory: &Path,
    ) -> Result<Vec<ToolTempDirectory>, ScratchSpaceError>;
    fn retire_tool_temp(&self, tool_temp: &Path) -> Result<(), ScratchSpaceError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolTempDirectory {
    path: PathBuf,
    owner: Option<u32>,
    modified_epoch_seconds: u64,
}

impl ToolTempDirectory {
    pub fn ownerless(path: PathBuf) -> Self {
        Self {
            path,
            owner: None,
            modified_epoch_seconds: 0,
        }
    }

    pub fn owned(path: PathBuf, owner: u32) -> Self {
        Self {
            path,
            owner: Some(owner),
            modified_epoch_seconds: 0,
        }
    }

    pub fn modified_at(self, epoch_seconds: u64) -> Self {
        Self {
            modified_epoch_seconds: epoch_seconds,
            ..self
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn owner(&self) -> Option<u32> {
        self.owner
    }

    pub fn modified_epoch_seconds(&self) -> u64 {
        self.modified_epoch_seconds
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ScratchSpaceError {
    #[error("cannot {operation} {path}: {detail}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        detail: String,
    },
    #[error("{path} holds a mount point, unmount it before removing the directory")]
    HoldsMount { path: PathBuf },
    #[error("{path} is not a scratch directory, so Ringleader kept it")]
    Foreign { path: PathBuf },
    #[error("{path} is not a retired scratch directory, so Ringleader kept it")]
    NotRetired { path: PathBuf },
    #[error(
        "{path} is not a temp directory that a tool left in a scratch directory, so Ringleader kept it"
    )]
    NotToolTemp { path: PathBuf },
    #[error(
        "{path} is not a real directory, so Ringleader retires and purges nothing until it is replaced with one"
    )]
    LinkedRetiredRoot { path: PathBuf },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_mount_says_to_unmount_before_removing_the_directory() {
        let error = ScratchSpaceError::HoldsMount {
            path: PathBuf::from("/cache/ringleader/tmp/8718940d"),
        };

        assert!(error.to_string().contains("unmount it before removing"));
    }
}
