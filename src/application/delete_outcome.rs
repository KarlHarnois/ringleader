use std::path::{Path, PathBuf};

use super::delete_impact::DeleteImpact;
use super::deletion_guards::{BranchPresence, Measurement};
use crate::domain::Session;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteOutcome {
    tip: Option<String>,
    head: Option<String>,
    impact: DeleteImpact,
    branch: BranchPresence,
    kept: Option<(PathBuf, String)>,
}

impl DeleteOutcome {
    pub fn deleted(measurement: Measurement) -> Self {
        let commits = measurement.commits();
        let tip = commits.tip().map(str::to_string);
        let head = commits
            .head()
            .filter(|head| Some(*head) != commits.tip())
            .map(str::to_string);
        let branch = measurement.branch();
        Self {
            tip,
            head,
            branch,
            impact: measurement.into_impact(),
            kept: None,
        }
    }

    pub fn forgotten(session: &Session) -> Self {
        Self {
            tip: None,
            head: None,
            impact: DeleteImpact::default(),
            branch: BranchPresence::Present,
            kept: Some((
                session.worktree_path().to_path_buf(),
                session.branch().to_string(),
            )),
        }
    }

    pub fn tip(&self) -> Option<&str> {
        self.tip.as_deref()
    }

    pub fn head(&self) -> Option<&str> {
        self.head.as_deref()
    }

    pub fn impact(&self) -> &DeleteImpact {
        &self.impact
    }

    #[cfg(test)]
    pub fn branch(&self) -> BranchPresence {
        self.branch
    }

    pub fn kept(&self) -> Option<(&Path, &str)> {
        self.kept
            .as_ref()
            .map(|(path, branch)| (path.as_path(), branch.as_str()))
    }

    pub fn summary(&self, name: &str) -> String {
        if let Some((worktree, branch)) = self.kept() {
            return format!(
                "forgot {name}, left the worktree {} and the branch {branch} on disk, run ringleader doctor",
                worktree.display()
            );
        }
        match (self.branch, self.tip()) {
            (_, Some(tip)) => {
                let head = self
                    .head()
                    .map_or(String::new(), |head| format!(", worktree HEAD {head}"));
                format!(
                    "deleted {name}, branch tip {tip}{head}; recover with git branch <name> <sha>"
                )
            }
            (BranchPresence::Unborn, None) => format!("deleted {name}, the session had no commits"),
            _ => format!(
                "deleted {name}, the branch was already gone; an interrupted earlier delete logged its SHAs in app.log"
            ),
        }
    }
}
