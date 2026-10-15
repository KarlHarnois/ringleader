use super::branch::Branch;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BranchTips {
    tips: Vec<(Branch, String)>,
}

impl BranchTips {
    pub fn new(tips: Vec<(Branch, String)>) -> Self {
        Self { tips }
    }

    pub fn tip_of(&self, branch: &Branch) -> Option<&str> {
        self.tips
            .iter()
            .find(|(tipped, _)| tipped == branch)
            .map(|(_, commit)| commit.as_str())
    }

    pub fn first_of(&self, branches: &[Branch]) -> Option<&str> {
        branches.iter().find_map(|branch| self.tip_of(branch))
    }

    pub fn commits_of(&self, branches: &[Branch]) -> Vec<String> {
        branches
            .iter()
            .filter_map(|branch| self.tip_of(branch))
            .map(str::to_string)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TipsFixture;

    impl TipsFixture {
        fn main() -> Branch {
            Branch::Local("main".to_string())
        }

        fn master() -> Branch {
            Branch::Local("master".to_string())
        }

        fn missing() -> Branch {
            Branch::Local("trunk".to_string())
        }

        fn listed_master_first() -> BranchTips {
            BranchTips::new(vec![
                (Self::master(), "bbb".to_string()),
                (Self::main(), "aaa".to_string()),
            ])
        }
    }

    #[test]
    fn the_first_branch_asked_for_wins_whatever_the_listing_order() {
        let tips = TipsFixture::listed_master_first();

        assert_eq!(
            tips.first_of(&[TipsFixture::main(), TipsFixture::master()]),
            Some("aaa")
        );
    }

    #[test]
    fn a_missing_branch_is_skipped() {
        let tips = TipsFixture::listed_master_first();

        assert_eq!(
            tips.first_of(&[TipsFixture::missing(), TipsFixture::master()]),
            Some("bbb")
        );
    }

    #[test]
    fn commits_follow_the_order_asked_for_and_skip_missing_branches() {
        let tips = TipsFixture::listed_master_first();
        let asked = [
            TipsFixture::main(),
            TipsFixture::missing(),
            TipsFixture::master(),
        ];

        assert_eq!(tips.commits_of(&asked), ["aaa", "bbb"]);
    }
}
