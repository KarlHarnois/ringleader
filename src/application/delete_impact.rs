use std::path::PathBuf;

use super::ports::{DirtyState, ProcessInfo, UnmergedCommits};
use super::service_error::ServiceError;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeleteImpact {
    dirty_files: u64,
    nested_repos: Vec<PathBuf>,
    unmerged_commits: u64,
    processes: Vec<ProcessInfo>,
    working: bool,
}

impl DeleteImpact {
    pub fn measured(
        dirty: &DirtyState,
        commits: &UnmergedCommits,
        processes: Vec<ProcessInfo>,
    ) -> Self {
        Self {
            dirty_files: dirty.dirty_files(),
            nested_repos: dirty.nested_repos().to_vec(),
            unmerged_commits: commits.count(),
            processes,
            working: false,
        }
    }

    pub fn while_working(self, working: bool) -> Self {
        Self { working, ..self }
    }

    pub fn processes(&self) -> &[ProcessInfo] {
        &self.processes
    }

    pub fn needs_confirmation(&self) -> bool {
        self.dirty_files > 0
            || self.unmerged_commits > 0
            || !self.nested_repos.is_empty()
            || !self.processes.is_empty()
            || self.working
    }

    pub fn warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        if self.dirty_files > 0 {
            warnings.push(format!(
                "{} uncommitted files will be lost",
                self.dirty_files
            ));
        }
        for nested in &self.nested_repos {
            warnings.push(format!(
                "untracked nested repository {} will be deleted with its files, and its local commits when its .git is a directory",
                nested.display()
            ));
        }
        if self.unmerged_commits > 0 {
            warnings.push(format!(
                "{} commits exist on no other ref and will be lost",
                self.unmerged_commits
            ));
        }
        for process in &self.processes {
            warnings.push(format!(
                "process {process} runs in the worktree, delete sends it SIGTERM and stops if it survives"
            ));
        }
        if self.working {
            warnings.push("agent is actively working".to_string());
        }
        warnings.push("ignored files such as .env are deleted with the worktree".to_string());
        warnings
    }

    fn accepts(&self, current: &DeleteImpact) -> bool {
        let working_confirmed = self.working || !current.working;
        working_confirmed
            && self.covers_count(self.dirty_files, current.dirty_files)
            && self.covers_count(self.unmerged_commits, current.unmerged_commits)
            && current
                .nested_repos
                .iter()
                .all(|path| self.nested_repos.contains(path))
            && current.processes.iter().all(|process| {
                self.processes
                    .iter()
                    .any(|seen| seen.pid() == process.pid())
            })
    }

    fn covers_count(&self, seen: u64, current: u64) -> bool {
        let appeared = seen == 0 && current > 0;
        let grew_unconfirmed = !self.working && current > seen;
        !appeared && !grew_unconfirmed
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteConfirmation {
    seen: Option<DeleteImpact>,
    forget: bool,
}

impl DeleteConfirmation {
    pub fn confirmed(seen: DeleteImpact) -> Self {
        Self {
            seen: Some(seen),
            forget: false,
        }
    }

    pub fn force() -> Self {
        Self {
            seen: None,
            forget: false,
        }
    }

    pub fn forgetting(self) -> Self {
        Self {
            forget: true,
            ..self
        }
    }

    pub fn forgets(&self) -> bool {
        self.forget
    }

    pub fn covers_working(&self) -> bool {
        self.seen.as_ref().is_none_or(|seen| seen.working)
    }

    pub fn check(&self, current: &DeleteImpact) -> Result<(), ServiceError> {
        let Some(seen) = &self.seen else {
            return Ok(());
        };
        if seen.accepts(current) {
            return Ok(());
        }
        Err(ServiceError::GuardsChanged {
            message: format!("delete would lose work: {}", current.warnings().join("; ")),
            impact: Box::new(current.clone()),
        })
    }
}
