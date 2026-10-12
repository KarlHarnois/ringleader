use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::application::ports::{
    DirtyState, StartPoint, UnmergedCommits, Workspace, WorkspaceError, WorktreeEntry,
};
use crate::domain::{BaseBranch, DiffStat};

#[derive(Default)]
pub struct FakeWorkspace {
    state: Mutex<WorkspaceState>,
}

#[derive(Default)]
struct WorkspaceState {
    worktrees: Vec<(PathBuf, WorktreeEntry)>,
    branches: Vec<FakeBranch>,
    markers: Vec<(PathBuf, String)>,
    dirty: HashMap<PathBuf, DirtyState>,
    unmerged: HashMap<String, VecDeque<UnmergedCommits>>,
    diffs: HashMap<PathBuf, DiffStat>,
    start_point: Option<Result<StartPoint, WorkspaceError>>,
    invalid_repos: HashSet<PathBuf>,
    failures: HashMap<&'static str, WorkspaceError>,
    failed_create_leftover: Option<FailedCreateLeftover>,
    partial_removal: bool,
    calls: Vec<String>,
    top_levels: HashMap<PathBuf, PathBuf>,
    version: Option<String>,
}

#[derive(Clone)]
struct FakeBranch {
    repo: PathBuf,
    name: String,
    tip: String,
}

#[derive(Clone, Copy)]
enum FailedCreateLeftover {
    Worktree,
    LockedWorktree,
}

impl FakeWorkspace {
    pub const STATE_CHANGING: [&'static str; 6] = [
        "create_worktree",
        "remove_worktree",
        "unlock_worktree",
        "delete_branch",
        "rename_branch",
        "mark_branch_managed",
    ];
    pub const DEFAULT_START: &'static str = "0123456789abcdef0123456789abcdef01234567";

    pub fn with_branch(self, repo: &Path, branch: &str, tip: &str) -> Self {
        self.edit(|state| state.add_branch(repo, branch, tip));
        self
    }

    pub fn with_marker(self, repo: &Path, branch: &str) -> Self {
        self.edit(|state| state.add_marker(repo, branch));
        self
    }

    pub fn starting_orphan(self) -> Self {
        self.edit(|state| state.start_point = Some(Ok(StartPoint::Orphan)));
        self
    }

    pub fn with_worktree(self, repo: &Path, entry: WorktreeEntry) -> Self {
        self.edit(|state| state.worktrees.push((repo.to_path_buf(), entry)));
        self
    }

    pub fn with_dirty(self, worktree: &Path, dirty: DirtyState) -> Self {
        self.edit(|state| state.dirty.insert(worktree.to_path_buf(), dirty));
        self
    }

    pub fn with_unmerged(self, branch: &str, reads: Vec<UnmergedCommits>) -> Self {
        self.edit(|state| state.unmerged.insert(branch.to_string(), reads.into()));
        self
    }

    pub fn with_diff(self, worktree: &Path, diff: DiffStat) -> Self {
        self.edit(|state| state.diffs.insert(worktree.to_path_buf(), diff));
        self
    }

    pub fn with_invalid_repo(self, repo: &Path) -> Self {
        self.edit(|state| state.invalid_repos.insert(repo.to_path_buf()));
        self
    }

    pub fn failing(self, operation: &'static str, error: WorkspaceError) -> Self {
        self.edit(|state| state.failures.insert(operation, error));
        self
    }

    pub fn leaving_worktree_after_failed_create(self) -> Self {
        self.edit(|state| state.failed_create_leftover = Some(FailedCreateLeftover::Worktree));
        self
    }

    pub fn leaving_locked_worktree_after_failed_create(self) -> Self {
        self.edit(|state| {
            state.failed_create_leftover = Some(FailedCreateLeftover::LockedWorktree)
        });
        self
    }

    pub fn with_top_level(self, path: &Path, top_level: &Path) -> Self {
        self.edit(|state| {
            state
                .top_levels
                .insert(path.to_path_buf(), top_level.to_path_buf())
        });
        self
    }

    pub fn with_version(self, version: &str) -> Self {
        self.edit(|state| state.version = Some(version.to_string()));
        self
    }

    pub fn removing_partially(self) -> Self {
        self.edit(|state| state.partial_removal = true);
        self
    }

    pub fn calls(&self) -> Vec<String> {
        self.state.lock().unwrap().calls.clone()
    }

    pub fn state_changing_calls(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter(|call| {
                Self::STATE_CHANGING
                    .iter()
                    .any(|name| call.starts_with(name))
            })
            .collect()
    }

    pub fn has_branch(&self, repo: &Path, branch: &str) -> bool {
        self.state
            .lock()
            .unwrap()
            .find_branch(repo, branch)
            .is_some()
    }

    pub fn has_marker(&self, repo: &Path, branch: &str) -> bool {
        self.state.lock().unwrap().is_marked(repo, branch)
    }

    pub fn lists_worktree(&self, repo: &Path, worktree: &Path) -> bool {
        self.state
            .lock()
            .unwrap()
            .find_worktree(repo, worktree)
            .is_some()
    }

    fn edit<T>(&self, change: impl FnOnce(&mut WorkspaceState) -> T) -> T {
        change(&mut self.state.lock().unwrap())
    }

    fn call(&self, operation: &'static str, detail: String) -> Result<(), WorkspaceError> {
        self.edit(|state| {
            state.calls.push(format!("{operation} {detail}"));
            match state.failures.get(operation) {
                Some(error) => Err(error.clone()),
                None => Ok(()),
            }
        })
    }
}

impl WorkspaceState {
    fn add_branch(&mut self, repo: &Path, name: &str, tip: &str) {
        self.branches.push(FakeBranch {
            repo: repo.to_path_buf(),
            name: name.to_string(),
            tip: tip.to_string(),
        });
    }

    fn add_marker(&mut self, repo: &Path, branch: &str) {
        if !self.is_marked(repo, branch) {
            self.markers.push((repo.to_path_buf(), branch.to_string()));
        }
    }

    fn is_marked(&self, repo: &Path, branch: &str) -> bool {
        self.markers
            .iter()
            .any(|(owner, name)| owner == repo && name == branch)
    }

    fn add_worktree(&mut self, repo: &Path, worktree: &Path, branch: &str, start: &StartPoint) {
        let entry = WorktreeEntry::new(worktree).on_branch(branch);
        let entry = match start {
            StartPoint::Commit(sha) => entry.at_head(sha),
            StartPoint::Orphan => entry,
        };
        self.worktrees.push((repo.to_path_buf(), entry));
        if let StartPoint::Commit(sha) = start {
            self.add_branch(repo, branch, sha);
        }
    }

    fn find_branch(&self, repo: &Path, name: &str) -> Option<&FakeBranch> {
        self.branches
            .iter()
            .find(|branch| branch.repo == repo && branch.name == name)
    }

    fn find_worktree(&self, repo: &Path, worktree: &Path) -> Option<&WorktreeEntry> {
        self.worktrees
            .iter()
            .find(|(owner, entry)| owner == repo && entry.path() == worktree)
            .map(|(_, entry)| entry)
    }

    fn worktree_head(&self, repo: &Path, worktree: &Path) -> Option<String> {
        self.find_worktree(repo, worktree)
            .and_then(|entry| entry.head().map(str::to_string))
    }

    fn next_unmerged(&mut self, repo: &Path, branch: &str, worktree: &Path) -> UnmergedCommits {
        if let Some(reads) = self.unmerged.get_mut(branch) {
            let read = if reads.len() > 1 {
                reads.pop_front()
            } else {
                reads.front().cloned()
            };
            if let Some(read) = read {
                return read;
            }
        }
        let tip = self
            .find_branch(repo, branch)
            .map(|found| found.tip.clone());
        let head = self.worktree_head(repo, worktree);
        UnmergedCommits::new(0, tip.as_deref(), head.as_deref())
    }

    fn leave_failed_create(
        &mut self,
        repo: &Path,
        worktree: &Path,
        branch: &str,
        start: &StartPoint,
    ) {
        let Some(leftover) = self.failed_create_leftover else {
            return;
        };
        self.add_worktree(repo, worktree, branch, start);
        if let FailedCreateLeftover::LockedWorktree = leftover {
            let (_, entry) = self.worktrees.pop().unwrap();
            self.worktrees.push((repo.to_path_buf(), entry.locked()));
        }
    }
}

impl Workspace for FakeWorkspace {
    fn create_worktree(
        &self,
        repo: &Path,
        worktree: &Path,
        branch: &str,
        start: &StartPoint,
    ) -> Result<(), WorkspaceError> {
        let detail = format!("{} {}", worktree.display(), branch);
        if let Err(error) = self.call("create_worktree", detail) {
            self.edit(|state| state.leave_failed_create(repo, worktree, branch, start));
            return Err(error);
        }
        self.edit(|state| state.add_worktree(repo, worktree, branch, start));
        Ok(())
    }

    fn remove_worktree(&self, repo: &Path, worktree: &Path) -> Result<(), WorkspaceError> {
        let result = self.call("remove_worktree", worktree.display().to_string());
        self.edit(|state| {
            if result.is_ok() || state.partial_removal {
                state
                    .worktrees
                    .retain(|(owner, entry)| !(owner == repo && entry.path() == worktree));
            }
        });
        result
    }

    fn unlock_worktree(&self, repo: &Path, worktree: &Path) -> Result<(), WorkspaceError> {
        self.call("unlock_worktree", worktree.display().to_string())?;
        self.edit(|state| {
            for (owner, entry) in state.worktrees.iter_mut() {
                if owner == repo && entry.path() == worktree {
                    *entry = entry.clone().unlocked();
                }
            }
        });
        Ok(())
    }

    fn list_worktrees(&self, repo: &Path) -> Result<Vec<WorktreeEntry>, WorkspaceError> {
        self.call("list_worktrees", repo.display().to_string())?;
        Ok(self.edit(|state| {
            state
                .worktrees
                .iter()
                .filter(|(owner, _)| owner == repo)
                .map(|(_, entry)| entry.clone())
                .collect()
        }))
    }

    fn list_worktree_dirs(&self, root: &Path) -> Result<Vec<PathBuf>, WorkspaceError> {
        self.call("list_worktree_dirs", root.display().to_string())?;
        Ok(self.edit(|state| {
            state
                .worktrees
                .iter()
                .map(|(_, entry)| entry.path().to_path_buf())
                .filter(|path| path.starts_with(root))
                .collect()
        }))
    }

    fn delete_branch(
        &self,
        repo: &Path,
        branch: &str,
        expected_tip: Option<&str>,
    ) -> Result<(), WorkspaceError> {
        let detail = format!("{branch} {}", expected_tip.unwrap_or("-"));
        self.call("delete_branch", detail)?;
        self.edit(|state| {
            let current_tip = state
                .find_branch(repo, branch)
                .map(|found| found.tip.clone());
            if expected_tip.is_some() && current_tip.as_deref() != expected_tip {
                return Err(WorkspaceError::Failed {
                    subcommand: "update-ref".to_string(),
                    status: "exit status: 1".to_string(),
                    stderr: "cannot lock ref".to_string(),
                });
            }
            state
                .branches
                .retain(|found| !(found.repo == repo && found.name == branch));
            state
                .markers
                .retain(|(owner, name)| !(owner == repo && name == branch));
            Ok(())
        })
    }

    fn rename_branch(&self, location: &Path, old: &str, new: &str) -> Result<(), WorkspaceError> {
        self.call(
            "rename_branch",
            format!("{} {old} {new}", location.display()),
        )?;
        self.edit(|state| {
            for branch in state
                .branches
                .iter_mut()
                .filter(|branch| branch.name == old)
            {
                branch.name = new.to_string();
            }
            for (_, entry) in state.worktrees.iter_mut() {
                if entry.branch() == Some(old) {
                    *entry = entry.clone().on_branch(new);
                }
            }
            for (_, name) in state.markers.iter_mut().filter(|(_, name)| name == old) {
                *name = new.to_string();
            }
        });
        Ok(())
    }

    fn branch_exists(&self, repo: &Path, branch: &str) -> Result<bool, WorkspaceError> {
        self.call("branch_exists", branch.to_string())?;
        Ok(self.edit(|state| {
            let named_by_worktree = state
                .worktrees
                .iter()
                .any(|(owner, entry)| owner == repo && entry.branch() == Some(branch));
            named_by_worktree || state.find_branch(repo, branch).is_some()
        }))
    }

    fn mark_branch_managed(&self, repo: &Path, branch: &str) -> Result<(), WorkspaceError> {
        self.call("mark_branch_managed", branch.to_string())?;
        self.edit(|state| state.add_marker(repo, branch));
        Ok(())
    }

    fn is_branch_managed(&self, repo: &Path, branch: &str) -> Result<bool, WorkspaceError> {
        self.call("is_branch_managed", branch.to_string())?;
        Ok(self.edit(|state| state.is_marked(repo, branch)))
    }

    fn list_managed_branches(&self, repo: &Path) -> Result<Vec<String>, WorkspaceError> {
        self.call("list_managed_branches", repo.display().to_string())?;
        Ok(self.edit(|state| {
            state
                .branches
                .iter()
                .filter(|branch| branch.repo == repo && state.is_marked(repo, &branch.name))
                .map(|branch| branch.name.clone())
                .collect()
        }))
    }

    fn resolve_start_point(
        &self,
        repo: &Path,
        _base_branch: Option<&BaseBranch>,
    ) -> Result<StartPoint, WorkspaceError> {
        self.call("resolve_start_point", repo.display().to_string())?;
        self.edit(|state| state.start_point.clone())
            .unwrap_or_else(|| Ok(StartPoint::Commit(Self::DEFAULT_START.to_string())))
    }

    fn dirty_state(&self, worktree: &Path) -> Result<DirtyState, WorkspaceError> {
        self.call("dirty_state", worktree.display().to_string())?;
        Ok(self.edit(|state| state.dirty.get(worktree).cloned().unwrap_or_default()))
    }

    fn unmerged_commits(
        &self,
        repo: &Path,
        branch: &str,
        worktree: &Path,
    ) -> Result<UnmergedCommits, WorkspaceError> {
        self.call("unmerged_commits", branch.to_string())?;
        Ok(self.edit(|state| state.next_unmerged(repo, branch, worktree)))
    }

    fn diff_stat(&self, worktree: &Path, _base_commit: &str) -> Result<DiffStat, WorkspaceError> {
        self.call("diff_stat", worktree.display().to_string())?;
        Ok(self.edit(|state| state.diffs.get(worktree).copied().unwrap_or_default()))
    }

    fn validate_repo(&self, path: &Path) -> Result<PathBuf, WorkspaceError> {
        self.call("validate_repo", path.display().to_string())?;
        if self.edit(|state| state.invalid_repos.contains(path)) {
            return Err(WorkspaceError::NotARepository {
                path: path.to_path_buf(),
                detail: "fatal: not a git repository".to_string(),
            });
        }
        Ok(self.edit(|state| {
            state
                .top_levels
                .get(path)
                .cloned()
                .unwrap_or_else(|| path.to_path_buf())
        }))
    }

    fn version(&self) -> Result<String, WorkspaceError> {
        self.call("version", String::new())?;
        Ok(self.edit(|state| {
            state
                .version
                .clone()
                .unwrap_or_else(|| "2.54.0".to_string())
        }))
    }
}
