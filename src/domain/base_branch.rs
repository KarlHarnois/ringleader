use std::fmt;

use super::branch::Branch;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BaseBranch(String);

impl BaseBranch {
    pub fn new(name: &str) -> Option<Self> {
        let is_blank = name.trim().is_empty();
        (!is_blank).then(|| Self(name.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn branches(&self) -> [Branch; 2] {
        [
            Branch::Local(self.0.clone()),
            Branch::RemoteTracking(self.0.clone()),
        ]
    }

    pub fn origin_copy(&self) -> Branch {
        Branch::origin_copy_of(&self.0)
    }

    pub fn branches_and_origin_copy(&self) -> Vec<Branch> {
        self.branches()
            .into_iter()
            .chain([self.origin_copy()])
            .collect()
    }
}

impl fmt::Display for BaseBranch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct BaseFixture;

    impl BaseFixture {
        fn develop() -> BaseBranch {
            BaseBranch::new("develop").unwrap()
        }
    }

    #[test]
    fn a_blank_name_is_no_base_branch() {
        assert_eq!(BaseBranch::new("  "), None);
    }

    #[test]
    fn a_base_branch_is_looked_up_as_local_before_remote_tracking() {
        assert_eq!(
            BaseFixture::develop().branches(),
            [
                Branch::Local("develop".to_string()),
                Branch::RemoteTracking("develop".to_string())
            ]
        );
    }

    #[test]
    fn a_remote_tracking_name_keeps_its_remote() {
        let base = BaseBranch::new("origin/main").unwrap();

        assert_eq!(
            base.branches()[1],
            Branch::RemoteTracking("origin/main".to_string())
        );
    }

    #[test]
    fn the_origin_copy_follows_the_local_and_remote_tracking_branches() {
        assert_eq!(
            BaseFixture::develop().branches_and_origin_copy(),
            [
                Branch::Local("develop".to_string()),
                Branch::RemoteTracking("develop".to_string()),
                Branch::RemoteTracking("origin/develop".to_string())
            ]
        );
    }
}
