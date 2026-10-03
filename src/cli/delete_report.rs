use crate::application::{BranchPresence, DeleteImpact, DeleteOutcome};

pub struct DeleteReport<'a> {
    name: &'a str,
}

impl<'a> DeleteReport<'a> {
    pub fn new(name: &'a str) -> Self {
        Self { name }
    }

    pub fn refusal(&self, impact: &DeleteImpact) -> Vec<String> {
        let mut lines = vec![format!("deleting {} would lose work:", self.name)];
        lines.extend(
            impact
                .warnings()
                .iter()
                .map(|warning| format!("  - {warning}")),
        );
        lines.push("pass --force to delete anyway".to_string());
        lines
    }

    pub fn warnings(&self, impact: &DeleteImpact) -> Vec<String> {
        impact
            .warnings()
            .iter()
            .map(|warning| format!("warning: {warning}"))
            .collect()
    }

    pub fn success(&self, outcome: &DeleteOutcome) -> String {
        if let Some((worktree, branch)) = outcome.kept() {
            return format!(
                "forgot {}, left the worktree {} and the branch {} on disk, run ringleader doctor",
                self.name,
                worktree.display(),
                branch
            );
        }
        match (outcome.branch(), outcome.tip()) {
            (_, Some(tip)) => self.deleted_with_tip(tip, outcome.head()),
            (BranchPresence::Unborn, None) => {
                format!("deleted {}, the session had no commits", self.name)
            }
            _ => format!(
                "deleted {}, the branch was already gone; an interrupted earlier delete logged its SHAs in app.log",
                self.name
            ),
        }
    }

    fn deleted_with_tip(&self, tip: &str, head: Option<&str>) -> String {
        let head = head.map_or(String::new(), |head| format!(", worktree HEAD {head}"));
        format!(
            "deleted {}, branch tip {tip}{head}; recover with git branch <name> <sha>",
            self.name
        )
    }
}
