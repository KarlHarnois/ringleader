use std::iter;

use super::base_branch_refs::BaseBranchRefs;
use crate::domain::BaseBranch;

pub struct BaseBranchLadder {
    origin_default: Option<String>,
}

impl BaseBranchLadder {
    pub const ORIGIN_HEAD: &'static str = "refs/remotes/origin/HEAD";
    const ORIGIN_BRANCH_PREFIX: &'static str = "refs/remotes/origin/";
    const LOCAL_BRANCH_PREFIX: &'static str = "refs/heads/";
    const CONVENTIONAL_DEFAULT_BRANCHES: [&'static str; 2] =
        ["refs/heads/main", "refs/heads/master"];

    pub fn new(origin_default: Option<String>) -> Self {
        Self { origin_default }
    }

    pub fn rung_lookup_refs() -> impl Iterator<Item = &'static str> {
        iter::once(Self::ORIGIN_HEAD).chain(Self::CONVENTIONAL_DEFAULT_BRANCHES)
    }

    pub fn candidates(&self) -> Vec<String> {
        let conventional = Self::CONVENTIONAL_DEFAULT_BRANCHES.map(String::from);
        self.origin_rung().into_iter().chain(conventional).collect()
    }

    pub fn origin_candidates(&self) -> Vec<String> {
        let conventional = Self::CONVENTIONAL_DEFAULT_BRANCHES
            .into_iter()
            .filter_map(Self::origin_counterpart);
        self.origin_default
            .clone()
            .into_iter()
            .chain(conventional)
            .collect()
    }

    pub fn origin_branch(origin_ref: &str) -> Option<BaseBranch> {
        origin_ref
            .strip_prefix(Self::ORIGIN_BRANCH_PREFIX)
            .and_then(BaseBranch::new)
    }

    pub fn first_rung(&self, existing_branches: &[&str]) -> Vec<String> {
        match self.origin_default {
            Some(_) => self.origin_rung(),
            None => Self::first_conventional_branch_in(existing_branches)
                .map(Self::with_origin_copy)
                .unwrap_or_default(),
        }
    }

    fn origin_rung(&self) -> Vec<String> {
        let local_default = self
            .origin_default
            .as_deref()
            .and_then(Self::local_counterpart);
        local_default
            .into_iter()
            .chain(self.origin_default.clone())
            .collect()
    }

    fn first_conventional_branch_in(existing_branches: &[&str]) -> Option<String> {
        Self::CONVENTIONAL_DEFAULT_BRANCHES
            .into_iter()
            .find(|branch| existing_branches.contains(branch))
            .map(String::from)
    }

    fn with_origin_copy(local_ref: String) -> Vec<String> {
        let origin_copy = Self::origin_counterpart(&local_ref);
        iter::once(local_ref).chain(origin_copy).collect()
    }

    fn origin_counterpart(local_ref: &str) -> Option<String> {
        local_ref
            .strip_prefix(Self::LOCAL_BRANCH_PREFIX)
            .and_then(BaseBranch::new)
            .map(|branch| BaseBranchRefs::new(&branch).origin_ref())
    }

    fn local_counterpart(origin_ref: &str) -> Option<String> {
        Self::origin_branch(origin_ref).map(|branch| BaseBranchRefs::new(&branch).local_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_origin_default_leads_with_its_local_branch_before_it() {
        let ladder = BaseBranchLadder::new(Some("refs/remotes/origin/trunk".to_string()));

        assert_eq!(
            ladder.candidates(),
            [
                "refs/heads/trunk",
                "refs/remotes/origin/trunk",
                "refs/heads/main",
                "refs/heads/master"
            ]
        );
    }

    #[test]
    fn with_an_origin_default_the_first_rung_is_its_local_and_origin_pair() {
        let ladder = BaseBranchLadder::new(Some("refs/remotes/origin/trunk".to_string()));

        assert_eq!(
            ladder.first_rung(&["refs/heads/main"]),
            ["refs/heads/trunk", "refs/remotes/origin/trunk"]
        );
    }

    #[test]
    fn without_an_origin_default_the_first_rung_is_main_and_its_origin_copy_when_both_exist() {
        let ladder = BaseBranchLadder::new(None);

        assert_eq!(
            ladder.first_rung(&["refs/heads/master", "refs/heads/main"]),
            ["refs/heads/main", "refs/remotes/origin/main"]
        );
    }

    #[test]
    fn without_an_origin_default_the_first_rung_falls_to_master_when_main_is_missing() {
        let ladder = BaseBranchLadder::new(None);

        assert_eq!(
            ladder.first_rung(&["refs/heads/master"]),
            ["refs/heads/master", "refs/remotes/origin/master"]
        );
    }

    #[test]
    fn the_origin_candidates_lead_with_the_origin_default() {
        let ladder = BaseBranchLadder::new(Some("refs/remotes/origin/trunk".to_string()));

        assert_eq!(
            ladder.origin_candidates(),
            [
                "refs/remotes/origin/trunk",
                "refs/remotes/origin/main",
                "refs/remotes/origin/master"
            ]
        );
    }

    #[test]
    fn an_origin_ref_names_its_branch_on_origin() {
        let branch = BaseBranchLadder::origin_branch("refs/remotes/origin/trunk");

        assert_eq!(branch, BaseBranch::new("trunk"));
    }

    #[test]
    fn a_local_ref_names_no_branch_on_origin() {
        assert_eq!(BaseBranchLadder::origin_branch("refs/heads/trunk"), None);
    }

    #[test]
    fn without_an_origin_default_only_main_and_master_are_candidates() {
        let ladder = BaseBranchLadder::new(None);

        assert_eq!(
            ladder.candidates(),
            ["refs/heads/main", "refs/heads/master"]
        );
    }
}
