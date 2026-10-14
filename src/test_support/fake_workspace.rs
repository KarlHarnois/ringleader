use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::CallHooks;
use crate::application::ports::{
    DirtyState, Head, MergeOutcome, StartPoint, UnmergedCommits, Workspace, WorkspaceError,
    WorktreeEntry,
};
use crate::domain::{BaseBranch, Branch, BranchTips, DiffStat, MergeBases};

#[derive(Default)]
pub struct FakeWorkspace {
    state: Mutex<WorkspaceState>,
    hooks: CallHooks,
}

#[derive(Default)]
struct WorkspaceState {
    worktrees: Vec<(PathBuf, WorktreeEntry)>,
    branches: Vec<FakeBranch>,
    markers: Vec<(PathBuf, String)>,
    published: Vec<(PathBuf, String)>,
    dirty: HashMap<PathBuf, DirtyState>,
    unmerged: HashMap<String, VecDeque<UnmergedCommits>>,
    diffs: HashMap<PathBuf, DiffStat>,
    tips: Vec<(PathBuf, Branch, String)>,
    without_default_tips: bool,
    remote_defaults: Vec<(PathBuf, String)>,
    orphan_start: bool,
    heads: Vec<(PathBuf, Head)>,
    tip_requests: Vec<(PathBuf, Vec<Branch>)>,
    merge_bases: HashMap<PathBuf, Vec<String>>,
    merge_base_requests: Vec<Vec<String>>,
    diff_bases: Vec<StartPoint>,
    merge_outcome: Option<MergeOutcome>,
    invalid_repos: HashSet<PathBuf>,
    failures: HashMap<&'static str, WorkspaceError>,
    failed_create_leftover: Option<FailedCreateLeftover>,
    partial_removal: bool,
    calls: Vec<String>,
    missing_on_origin: Vec<String>,
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
    pub const STATE_CHANGING: [&'static str; 9] = [
        "create_worktree",
        "remove_worktree",
        "unlock_worktree",
        "delete_branch",
        "rename_branch",
        "drop_stale_upstream",
        "mark_branch_managed",
        "fetch_upstream",
        "merge_upstream",
    ];
    pub const DEFAULT_START: &'static str = "0123456789abcdef0123456789abcdef01234567";
    pub const DEFAULT_BRANCH: &'static str = "main";

    pub fn with_branch(self, repo: &Path, branch: &str, tip: &str) -> Self {
        self.edit(|state| state.add_branch(repo, branch, tip));
        self
    }

    pub fn with_marker(self, repo: &Path, branch: &str) -> Self {
        self.edit(|state| state.add_marker(repo, branch));
        self
    }

    pub fn without_marker(self, repo: &Path, branch: &str) -> Self {
        self.edit(|state| {
            state
                .markers
                .retain(|(owner, name)| !(owner == repo && name == branch))
        });
        self
    }

    pub fn with_published_branch(self, repo: &Path, branch: &str) -> Self {
        self.edit(|state| {
            state
                .published
                .push((repo.to_path_buf(), branch.to_string()))
        });
        self
    }

    pub fn starting_orphan(self) -> Self {
        self.edit(|state| state.orphan_start = true);
        self.without_branch_tips()
    }

    pub fn with_branch_tip(self, location: &Path, branch: Branch, commit: &str) -> Self {
        self.edit(|state| {
            state
                .tips
                .push((location.to_path_buf(), branch, commit.to_string()))
        });
        self
    }

    pub fn with_branch_tips(self, location: &Path, tips: &[(Branch, &str)]) -> Self {
        tips.iter().fold(self, |workspace, (branch, tip)| {
            workspace.with_branch_tip(location, branch.clone(), tip)
        })
    }

    pub fn without_branch_tips(self) -> Self {
        self.edit(|state| state.without_default_tips = true);
        self
    }

    pub fn with_remote_default(self, location: &Path, branch: &str) -> Self {
        self.edit(|state| {
            state
                .remote_defaults
                .push((location.to_path_buf(), branch.to_string()))
        });
        self
    }

    pub fn with_head(self, location: &Path, head: Head) -> Self {
        self.edit(|state| state.heads.push((location.to_path_buf(), head)));
        self
    }

    pub fn with_merge_bases(self, worktree: &Path, bases: &[&str]) -> Self {
        let bases = bases.iter().map(ToString::to_string).collect();
        self.edit(|state| state.merge_bases.insert(worktree.to_path_buf(), bases));
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
        self.change_diff(worktree, diff);
        self
    }

    pub fn change_diff(&self, worktree: &Path, diff: DiffStat) {
        self.edit(|state| state.diffs.insert(worktree.to_path_buf(), diff));
    }

    pub fn with_merge_outcome(self, outcome: MergeOutcome) -> Self {
        self.edit(|state| state.merge_outcome = Some(outcome));
        self
    }

    pub fn with_invalid_repo(self, repo: &Path) -> Self {
        self.edit(|state| state.invalid_repos.insert(repo.to_path_buf()));
        self
    }

    pub fn without_branch_on_origin(self, branch: &str) -> Self {
        self.edit(|state| state.missing_on_origin.push(branch.to_string()));
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

    pub fn on_call(self, operation: &'static str, hook: impl Fn() + Send + Sync + 'static) -> Self {
        self.hooks.add(operation, hook);
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

    pub fn branch_tip_requests(&self) -> Vec<(PathBuf, Vec<Branch>)> {
        self.edit(|state| state.tip_requests.clone())
    }

    pub fn merge_base_requests(&self) -> Vec<Vec<String>> {
        self.edit(|state| state.merge_base_requests.clone())
    }

    pub fn diff_bases(&self) -> Vec<StartPoint> {
        self.edit(|state| state.diff_bases.clone())
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
        self.hooks.run(operation);
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
    fn fetch_from_origin(
        &mut self,
        worktree: &Path,
        branch: &BaseBranch,
    ) -> Result<(), WorkspaceError> {
        if !self.missing_on_origin.contains(&branch.to_string()) {
            return Ok(());
        }
        let origin_copy = branch.origin_copy();
        self.tips
            .retain(|(owner, tipped, _)| !(owner == worktree && *tipped == origin_copy));
        Err(WorkspaceError::NotOnOrigin {
            branch: branch.to_string(),
        })
    }

    fn add_branch(&mut self, repo: &Path, name: &str, tip: &str) {
        self.branches.push(FakeBranch {
            repo: repo.to_path_buf(),
            name: name.to_string(),
            tip: tip.to_string(),
        });
    }

    fn tips_at(&self, location: &Path) -> Vec<(Branch, String)> {
        let listed: Vec<(Branch, String)> = self
            .tips
            .iter()
            .filter(|(owner, _, _)| owner == location)
            .map(|(_, branch, commit)| (branch.clone(), commit.clone()))
            .collect();
        if !listed.is_empty() || self.without_default_tips {
            return listed;
        }
        let default_branch = Branch::Local(FakeWorkspace::DEFAULT_BRANCH.to_string());
        let origin_copy = Branch::origin_copy_of(FakeWorkspace::DEFAULT_BRANCH);
        [default_branch, origin_copy]
            .map(|branch| (branch, FakeWorkspace::DEFAULT_START.to_string()))
            .to_vec()
    }

    fn head_at(&self, location: &Path) -> Head {
        let recorded = self.heads.iter().find(|(owner, _)| owner == location);
        recorded.map_or_else(|| self.default_head(), |(_, head)| head.clone())
    }

    fn default_head(&self) -> Head {
        match self.orphan_start {
            true => Head::Unborn,
            false => Head::Detached,
        }
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

    fn drop_stale_upstream(
        &self,
        location: &Path,
        old: &str,
        new: &str,
    ) -> Result<(), WorkspaceError> {
        self.call(
            "drop_stale_upstream",
            format!("{} {old} {new}", location.display()),
        )
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

    fn first_branch_under(
        &self,
        repo: &Path,
        branch: &str,
    ) -> Result<Option<String>, WorkspaceError> {
        self.call("first_branch_under", branch.to_string())?;
        let directory = format!("{branch}/");
        Ok(self.edit(|state| {
            state
                .branches
                .iter()
                .find(|nested| nested.repo == repo && nested.name.starts_with(&directory))
                .map(|nested| nested.name.clone())
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

    fn branch_marking_command(&self, repo: &Path, branch: &str) -> String {
        format!("mark-managed {} {branch}", repo.display())
    }

    fn is_branch_published(&self, repo: &Path, branch: &str) -> Result<bool, WorkspaceError> {
        self.call("is_branch_published", branch.to_string())?;
        Ok(self.edit(|state| {
            state
                .published
                .iter()
                .any(|(owner, name)| owner == repo && name == branch)
        }))
    }

    fn head(&self, location: &Path) -> Result<Head, WorkspaceError> {
        self.call("head", location.display().to_string())?;
        Ok(self.edit(|state| state.head_at(location)))
    }

    fn remote_default_branch(
        &self,
        location: &Path,
        remote: &str,
    ) -> Result<Option<BaseBranch>, WorkspaceError> {
        self.call(
            "remote_default_branch",
            format!("{} {remote}", location.display()),
        )?;
        Ok(self.edit(|state| {
            state
                .remote_defaults
                .iter()
                .find(|(owner, _)| owner == location)
                .and_then(|(_, branch)| BaseBranch::new(branch))
        }))
    }

    fn branch_tips(
        &self,
        location: &Path,
        branches: &[Branch],
    ) -> Result<BranchTips, WorkspaceError> {
        self.call("branch_tips", location.display().to_string())?;
        Ok(self.edit(|state| {
            state
                .tip_requests
                .push((location.to_path_buf(), branches.to_vec()));
            let known = state.tips_at(location);
            let tips = branches
                .iter()
                .filter_map(|branch| known.iter().find(|(tipped, _)| tipped == branch).cloned());
            BranchTips::new(tips.collect())
        }))
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

    fn merge_bases(
        &self,
        worktree: &Path,
        commits: &[String],
    ) -> Result<MergeBases, WorkspaceError> {
        self.call("merge_bases", worktree.display().to_string())?;
        Ok(self.edit(|state| {
            state.merge_base_requests.push(commits.to_vec());
            MergeBases::new(state.merge_bases.get(worktree).cloned().unwrap_or_default())
        }))
    }

    fn diff_stat(&self, worktree: &Path, base: &StartPoint) -> Result<DiffStat, WorkspaceError> {
        let diff = self.edit(|state| {
            state.diff_bases.push(base.clone());
            state.diffs.get(worktree).copied().unwrap_or_default()
        });
        self.call("diff_stat", worktree.display().to_string())?;
        Ok(diff)
    }

    fn fetch_upstream(
        &self,
        worktree: &Path,
        branch_on_origin: &BaseBranch,
    ) -> Result<(), WorkspaceError> {
        self.call(
            "fetch_upstream",
            format!("{} {branch_on_origin}", worktree.display()),
        )?;
        self.edit(|state| state.fetch_from_origin(worktree, branch_on_origin))
    }

    fn merge_upstream(
        &self,
        worktree: &Path,
        branch_on_origin: &BaseBranch,
        started_from: &str,
    ) -> Result<MergeOutcome, WorkspaceError> {
        self.call(
            "merge_upstream",
            format!("{} {branch_on_origin} {started_from}", worktree.display()),
        )?;
        Ok(self.edit(|state| state.merge_outcome.clone().unwrap_or(MergeOutcome::Merged)))
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
