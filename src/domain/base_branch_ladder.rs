use super::base_branch::BaseBranch;
use super::branch::Branch;
use super::branch_tips::BranchTips;

pub struct BaseBranchLadder {
    origin_default: Option<String>,
    session_branches: Vec<Branch>,
}

impl BaseBranchLadder {
    const CONVENTIONAL_DEFAULT_BRANCHES: [&'static str; 2] = ["main", "master"];

    pub fn new(origin_default: Option<String>) -> Self {
        Self {
            origin_default,
            session_branches: Vec::new(),
        }
    }

    pub fn skipping(self, session_branches: &[Branch]) -> Self {
        Self {
            session_branches: session_branches.to_vec(),
            ..self
        }
    }

    pub fn start_candidates(&self) -> Vec<Branch> {
        self.origin_rung()
            .into_iter()
            .chain(Self::conventional_branches())
            .filter(|branch| self.is_free(branch))
            .collect()
    }

    pub fn diff_lookups(&self) -> Vec<Branch> {
        match self.origin_default {
            Some(_) => self.origin_rung(),
            None => Self::conventional_branches()
                .iter()
                .flat_map(Self::with_origin_copy)
                .collect(),
        }
    }

    pub fn diff_rung(&self, tips: &BranchTips, checked_out: Option<&Branch>) -> Vec<Branch> {
        match self.origin_default {
            Some(_) => self
                .origin_rung()
                .into_iter()
                .filter(|branch| self.is_free(branch))
                .collect(),
            None => Self::conventional_branches()
                .into_iter()
                .find(|branch| {
                    Some(branch) != checked_out
                        && self.is_free(branch)
                        && tips.tip_of(branch).is_some()
                })
                .map(|branch| Self::with_origin_copy(&branch))
                .unwrap_or_default(),
        }
    }

    pub fn upstream_lookups(&self) -> Vec<Branch> {
        self.upstream_candidates()
            .iter()
            .map(BaseBranch::origin_copy)
            .collect()
    }

    pub fn upstream(&self, tips: &BranchTips) -> Option<BaseBranch> {
        self.upstream_candidates()
            .into_iter()
            .find(|candidate| tips.tip_of(&candidate.origin_copy()).is_some())
    }

    fn upstream_candidates(&self) -> Vec<BaseBranch> {
        self.origin_default
            .as_deref()
            .into_iter()
            .chain(Self::CONVENTIONAL_DEFAULT_BRANCHES)
            .filter_map(BaseBranch::new)
            .collect()
    }

    fn is_free(&self, branch: &Branch) -> bool {
        !self.session_branches.contains(branch)
    }

    fn origin_rung(&self) -> Vec<Branch> {
        self.origin_default
            .as_deref()
            .map(|name| Self::with_origin_copy(&Branch::Local(name.to_string())))
            .unwrap_or_default()
    }

    fn conventional_branches() -> [Branch; 2] {
        Self::CONVENTIONAL_DEFAULT_BRANCHES.map(|name| Branch::Local(name.to_string()))
    }

    fn with_origin_copy(local: &Branch) -> Vec<Branch> {
        vec![local.clone(), Branch::origin_copy_of(local.name())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct LadderFixture;

    impl LadderFixture {
        const COMMIT: &'static str = "0123456789abcdef0123456789abcdef01234567";

        fn local(name: &str) -> Branch {
            Branch::Local(name.to_string())
        }

        fn tracked(name: &str) -> Branch {
            Branch::RemoteTracking(name.to_string())
        }

        fn with_trunk() -> BaseBranchLadder {
            BaseBranchLadder::new(Some("trunk".to_string()))
        }

        fn without_origin_default() -> BaseBranchLadder {
            BaseBranchLadder::new(None)
        }

        fn tips_of(names: &[&str]) -> BranchTips {
            let tips = names
                .iter()
                .map(|name| (Self::local(name), Self::COMMIT.to_string()));
            BranchTips::new(tips.collect())
        }

        fn tracked_tips_of(names: &[&str]) -> BranchTips {
            let tips = names
                .iter()
                .map(|name| (Self::tracked(name), Self::COMMIT.to_string()));
            BranchTips::new(tips.collect())
        }
    }

    #[test]
    fn the_origin_default_leads_with_its_local_branch_before_it() {
        assert_eq!(
            LadderFixture::with_trunk().start_candidates(),
            [
                LadderFixture::local("trunk"),
                LadderFixture::tracked("origin/trunk"),
                LadderFixture::local("main"),
                LadderFixture::local("master")
            ]
        );
    }

    #[test]
    fn without_an_origin_default_only_main_and_master_are_start_candidates() {
        assert_eq!(
            LadderFixture::without_origin_default().start_candidates(),
            [LadderFixture::local("main"), LadderFixture::local("master")]
        );
    }

    #[test]
    fn with_an_origin_default_the_diff_looks_up_only_its_pair() {
        assert_eq!(
            LadderFixture::with_trunk().diff_lookups(),
            [
                LadderFixture::local("trunk"),
                LadderFixture::tracked("origin/trunk")
            ]
        );
    }

    #[test]
    fn without_an_origin_default_the_diff_looks_up_main_and_master_with_their_origin_copies() {
        assert_eq!(
            LadderFixture::without_origin_default().diff_lookups(),
            [
                LadderFixture::local("main"),
                LadderFixture::tracked("origin/main"),
                LadderFixture::local("master"),
                LadderFixture::tracked("origin/master")
            ]
        );
    }

    #[test]
    fn with_an_origin_default_the_diff_rung_is_its_local_and_origin_pair() {
        let tips = LadderFixture::tips_of(&["main"]);

        assert_eq!(
            LadderFixture::with_trunk().diff_rung(&tips, None),
            [
                LadderFixture::local("trunk"),
                LadderFixture::tracked("origin/trunk")
            ]
        );
    }

    #[test]
    fn without_an_origin_default_the_diff_rung_is_main_and_its_origin_copy_when_both_exist() {
        let tips = LadderFixture::tips_of(&["master", "main"]);

        assert_eq!(
            LadderFixture::without_origin_default().diff_rung(&tips, None),
            [
                LadderFixture::local("main"),
                LadderFixture::tracked("origin/main")
            ]
        );
    }

    #[test]
    fn without_an_origin_default_the_diff_rung_falls_to_master_when_main_is_missing() {
        let tips = LadderFixture::tips_of(&["master"]);

        assert_eq!(
            LadderFixture::without_origin_default().diff_rung(&tips, None),
            [
                LadderFixture::local("master"),
                LadderFixture::tracked("origin/master")
            ]
        );
    }

    #[test]
    fn the_checked_out_main_is_skipped_for_master() {
        let tips = LadderFixture::tips_of(&["main", "master"]);
        let checked_out = LadderFixture::local("main");

        assert_eq!(
            LadderFixture::without_origin_default().diff_rung(&tips, Some(&checked_out)),
            [
                LadderFixture::local("master"),
                LadderFixture::tracked("origin/master")
            ]
        );
    }

    #[test]
    fn a_session_branch_is_never_a_start_candidate() {
        let ladder =
            LadderFixture::without_origin_default().skipping(&[LadderFixture::local("main")]);

        assert_eq!(ladder.start_candidates(), [LadderFixture::local("master")]);
    }

    #[test]
    fn a_session_branch_named_like_the_origin_default_leaves_origin_copy_to_start_from() {
        let ladder = LadderFixture::with_trunk().skipping(&[LadderFixture::local("trunk")]);

        assert_eq!(
            ladder.start_candidates()[0],
            LadderFixture::tracked("origin/trunk")
        );
    }

    #[test]
    fn a_session_branch_is_never_a_diff_rung() {
        let tips = LadderFixture::tips_of(&["main", "master"]);
        let ladder =
            LadderFixture::without_origin_default().skipping(&[LadderFixture::local("main")]);

        assert_eq!(
            ladder.diff_rung(&tips, None),
            [
                LadderFixture::local("master"),
                LadderFixture::tracked("origin/master")
            ]
        );
    }

    #[test]
    fn without_main_or_master_the_diff_rung_is_empty() {
        let tips = LadderFixture::tips_of(&[]);

        assert!(
            LadderFixture::without_origin_default()
                .diff_rung(&tips, None)
                .is_empty()
        );
    }

    #[test]
    fn the_upstream_lookups_are_the_origin_copies_led_by_the_origin_default() {
        assert_eq!(
            LadderFixture::with_trunk().upstream_lookups(),
            [
                LadderFixture::tracked("origin/trunk"),
                LadderFixture::tracked("origin/main"),
                LadderFixture::tracked("origin/master")
            ]
        );
    }

    #[test]
    fn the_origin_default_is_the_upstream_over_main() {
        let tips = LadderFixture::tracked_tips_of(&["origin/trunk", "origin/main"]);

        assert_eq!(
            LadderFixture::with_trunk().upstream(&tips),
            BaseBranch::new("trunk")
        );
    }

    #[test]
    fn without_an_origin_default_the_first_conventional_branch_on_origin_is_the_upstream() {
        let tips = LadderFixture::tracked_tips_of(&["origin/master"]);

        assert_eq!(
            LadderFixture::without_origin_default().upstream(&tips),
            BaseBranch::new("master")
        );
    }

    #[test]
    fn a_local_branch_is_never_the_upstream() {
        let tips = LadderFixture::tips_of(&["main"]);

        assert_eq!(
            LadderFixture::without_origin_default().upstream(&tips),
            None
        );
    }
}
