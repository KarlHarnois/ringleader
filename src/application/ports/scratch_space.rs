use std::path::{Path, PathBuf};

use thiserror::Error;

pub trait ScratchSpace: Send + Sync {
    fn directory(&self, worktree: &Path) -> PathBuf;
    fn create(&self, worktree: &Path) -> Result<(), ScratchSpaceError>;
    fn remove(&self, worktree: &Path) -> Result<(), ScratchSpaceError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ScratchSpaceError {
    #[error("cannot {operation} the scratch directory {path}: {detail}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        detail: String,
    },
    #[error(
        "the scratch directory {path} holds a mount point, unmount it before removing the directory"
    )]
    HoldsMount { path: PathBuf },
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
