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
    kept: Kept,
    stray_session: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kept {
    Nothing,
    Forgotten(PathBuf, String),
    MainCheckout(PathBuf, String),
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
            kept: Kept::Nothing,
            stray_session: None,
        }
    }

    pub fn with_stray_session(self, warning: Option<String>) -> Self {
        Self {
            stray_session: warning,
            ..self
        }
    }

    pub fn forgotten(session: &Session) -> Self {
        Self::keeping(Kept::Forgotten(
            session.worktree_path().to_path_buf(),
            session.branch().to_string(),
        ))
    }

    pub fn checkout_kept(session: &Session) -> Self {
        Self::keeping(Kept::MainCheckout(
            session.worktree_path().to_path_buf(),
            session.branch().to_string(),
        ))
    }

    fn keeping(kept: Kept) -> Self {
        Self {
            tip: None,
            head: None,
            impact: DeleteImpact::default(),
            branch: BranchPresence::Present,
            kept,
            stray_session: None,
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

    pub fn stray_session(&self) -> Option<&str> {
        self.stray_session.as_deref()
    }

    pub fn kept(&self) -> Option<(&Path, &str)> {
        match &self.kept {
            Kept::Nothing => None,
            Kept::Forgotten(path, branch) | Kept::MainCheckout(path, branch) => {
                Some((path.as_path(), branch.as_str()))
            }
        }
    }

    pub fn summary(&self, name: &str) -> String {
        match &self.kept {
            Kept::Nothing => self.deleted_summary(name),
            Kept::Forgotten(worktree, branch) => format!(
                "forgot {name}, left the worktree {} and the branch {branch} on disk, run ringleader doctor",
                worktree.display()
            ),
            Kept::MainCheckout(checkout, branch) => format!(
                "deleted {name}, kept the main checkout {} and its branch {branch}",
                checkout.display()
            ),
        }
    }

    fn deleted_summary(&self, name: &str) -> String {
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
