use super::workspace::StartPoint;
use crate::domain::BaseBranch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffBase {
    start: StartPoint,
    base_branch: Option<BaseBranch>,
}

impl DiffBase {
    pub fn new(base_commit: &str, base_branch: Option<&BaseBranch>) -> Self {
        Self {
            start: StartPoint::from_base_commit(base_commit),
            base_branch: base_branch.cloned(),
        }
    }

    pub fn start(&self) -> &StartPoint {
        &self.start
    }

    pub fn base_branch(&self) -> Option<&BaseBranch> {
        self.base_branch.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_recorded_commit_starts_an_orphan() {
        let base = DiffBase::new("", None);

        assert_eq!(base.start(), &StartPoint::Orphan);
    }
}
