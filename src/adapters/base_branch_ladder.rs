use crate::domain::BaseBranch;

pub struct BaseBranchLadder {
    origin_default: Option<String>,
}

impl BaseBranchLadder {
    pub const ORIGIN_HEAD: &'static str = "refs/remotes/origin/HEAD";
    const ORIGIN_BRANCH_PREFIX: &'static str = "refs/remotes/origin/";
    const CONVENTIONAL_DEFAULT_BRANCHES: [&'static str; 2] =
        ["refs/heads/main", "refs/heads/master"];

    pub fn new(origin_default: Option<String>) -> Self {
        Self { origin_default }
    }

    pub fn candidates(&self) -> Vec<String> {
        let local_default = self
            .origin_default
            .as_deref()
            .and_then(Self::local_counterpart);
        local_default
            .into_iter()
            .chain(self.origin_default.clone())
            .chain(Self::CONVENTIONAL_DEFAULT_BRANCHES.map(String::from))
            .collect()
    }

    fn local_counterpart(origin_ref: &str) -> Option<String> {
        origin_ref
            .strip_prefix(Self::ORIGIN_BRANCH_PREFIX)
            .and_then(BaseBranch::new)
            .map(|branch| branch.local_ref())
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
    fn without_an_origin_default_only_main_and_master_are_candidates() {
        let ladder = BaseBranchLadder::new(None);

        assert_eq!(
            ladder.candidates(),
            ["refs/heads/main", "refs/heads/master"]
        );
    }
}
