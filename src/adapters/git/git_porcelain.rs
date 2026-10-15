use std::path::{Path, PathBuf};

use crate::adapters::filesystem::host_path::HostPath;
use crate::application::ports::{DirtyState, WorkspaceError, WorktreeEntry};
use crate::domain::FileChange;

pub struct GitPorcelain {
    text: String,
}

impl GitPorcelain {
    const UNBORN_HEAD: &'static str = "0000000000000000000000000000000000000000";
    const BRANCH_REF_PREFIX: &'static str = "refs/heads/";
    const UNTRACKED_MARKER: &'static str = "?? ";
    const GITLINK_MODE: &'static str = "160000 ";

    pub fn new(bytes: &[u8]) -> Self {
        Self {
            text: String::from_utf8_lossy(bytes).into_owned(),
        }
    }

    pub fn worktrees(&self) -> Vec<WorktreeEntry> {
        let mut entries: Vec<WorktreeEntry> = Vec::new();
        for field in self.text.split('\0') {
            if let Some(path) = field.strip_prefix("worktree ") {
                let entry = WorktreeEntry::new(Path::new(path));
                entries.push(if entries.is_empty() {
                    entry.main()
                } else {
                    entry
                });
            } else if let Some(entry) = entries.pop() {
                entries.push(Self::with_field(entry, field));
            }
        }
        entries
    }

    pub fn dirty_state(&self, worktree: &Path) -> DirtyState {
        let records: Vec<&str> = self.records().collect();
        let nested_repos: Vec<PathBuf> = records
            .iter()
            .filter_map(|record| record.strip_prefix(Self::UNTRACKED_MARKER))
            .filter_map(|path| path.strip_suffix('/'))
            .map(|path| worktree.join(path))
            .collect();
        DirtyState::present(records.len() as u64, nested_repos)
    }

    pub fn file_changes(&self) -> Vec<FileChange> {
        self.text
            .lines()
            .filter_map(|line| {
                let mut columns = line.split('\t');
                let insertions = columns.next()?;
                let deletions = columns.next()?;
                Some(Self::file_change(insertions, deletions))
            })
            .collect()
    }

    pub fn gitlink_paths(&self) -> Vec<&str> {
        self.records()
            .filter(|record| record.starts_with(Self::GITLINK_MODE))
            .filter_map(|record| record.split_once('\t'))
            .map(|(_, path)| path)
            .collect()
    }

    pub fn paths(&self) -> Vec<String> {
        self.records().map(str::to_string).collect()
    }

    pub fn record_count(&self) -> u64 {
        self.records().count() as u64
    }

    pub fn first_line(&self) -> &str {
        self.text.lines().next().unwrap_or("").trim()
    }

    pub fn lines(&self) -> Vec<&str> {
        self.text.lines().collect()
    }

    pub fn top_level(&self, path: &Path) -> Result<PathBuf, WorkspaceError> {
        let path = path.to_path_buf();
        match self.repo_kind() {
            Some(RepoKind::Repository(top_level)) => Ok(top_level),
            Some(RepoKind::LinkedWorktree(_)) => Err(WorkspaceError::LinkedWorktree { path }),
            Some(RepoKind::Submodule(_)) => Err(WorkspaceError::Submodule { path }),
            None => Err(WorkspaceError::NotARepository {
                path,
                detail: "git printed no top level".to_string(),
            }),
        }
    }

    pub fn repo_kind(&self) -> Option<RepoKind> {
        let canonical = |text: &str| HostPath::new(Path::new(text)).canonical_lenient();
        match self.lines().as_slice() {
            [_, _, _, superproject] => Some(RepoKind::Submodule(canonical(superproject))),
            [_, git_dir, common_dir] if git_dir != common_dir => {
                Some(RepoKind::LinkedWorktree(canonical(common_dir)))
            }
            [top_level, _, _] => Some(RepoKind::Repository(canonical(top_level))),
            _ => None,
        }
    }

    fn records(&self) -> impl Iterator<Item = &str> {
        self.text.split('\0').filter(|record| !record.is_empty())
    }

    fn with_field(entry: WorktreeEntry, field: &str) -> WorktreeEntry {
        if let Some(head) = field.strip_prefix("HEAD ") {
            if head == Self::UNBORN_HEAD {
                entry
            } else {
                entry.at_head(head)
            }
        } else if let Some(branch) = field.strip_prefix("branch ") {
            entry.on_branch(
                branch
                    .strip_prefix(Self::BRANCH_REF_PREFIX)
                    .unwrap_or(branch),
            )
        } else if field == "locked" || field.starts_with("locked ") {
            entry.locked()
        } else {
            entry
        }
    }

    fn file_change(insertions: &str, deletions: &str) -> FileChange {
        match (insertions.parse(), deletions.parse()) {
            (Ok(insertions), Ok(deletions)) => FileChange::Text {
                insertions,
                deletions,
            },
            _ => FileChange::Binary,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoKind {
    Repository(PathBuf),
    LinkedWorktree(PathBuf),
    Submodule(PathBuf),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::DiffStat;

    const NUMSTAT: &str = include_str!("../../../tests/fixtures/git-numstat.txt");
    const UNTRACKED: &str = include_str!("../../../tests/fixtures/git-untracked.txt");

    #[test]
    fn numstat_rows_accumulate_with_a_binary_file() {
        let changes = GitPorcelain::new(NUMSTAT.as_bytes()).file_changes();

        let diff = DiffStat::from_changes(changes, 0);

        assert_eq!(
            (diff.files(), diff.insertions(), diff.deletions()),
            (4, 129, 33)
        );
    }

    #[test]
    fn untracked_records_are_counted() {
        assert_eq!(GitPorcelain::new(UNTRACKED.as_bytes()).record_count(), 3);
    }

    #[test]
    fn nul_separated_paths_are_listed_in_order() {
        let listing = GitPorcelain::new(b"src/a.rs\0docs/b c.md\0");

        assert_eq!(listing.paths(), ["src/a.rs", "docs/b c.md"]);
    }

    #[test]
    fn the_first_worktree_is_the_main_one() {
        let listing = "worktree /repo\0HEAD abc\0branch refs/heads/main\0\0worktree /wt\0HEAD def\0branch refs/heads/ringleader/x\0locked\0\0";

        let entries = GitPorcelain::new(listing.as_bytes()).worktrees();

        assert!(entries[0].is_main());
        assert!(!entries[1].is_main());
        assert!(entries[1].is_locked());
        assert_eq!(entries[1].branch(), Some("ringleader/x"));
    }

    #[test]
    fn an_unborn_head_reads_as_no_head() {
        let listing = format!(
            "worktree /wt\0HEAD {}\0branch refs/heads/ringleader/x\0\0",
            GitPorcelain::UNBORN_HEAD
        );

        let entries = GitPorcelain::new(listing.as_bytes()).worktrees();

        assert_eq!(entries[0].head(), None);
    }

    #[test]
    fn an_untracked_directory_record_is_a_nested_repository() {
        let status = "?? notes.md\0?? vendor/tool/\0 M src/main.rs\0";

        let dirty = GitPorcelain::new(status.as_bytes()).dirty_state(Path::new("/wt"));

        assert_eq!(dirty.dirty_files(), 3);
        assert_eq!(dirty.nested_repos(), [PathBuf::from("/wt/vendor/tool")]);
    }
}
