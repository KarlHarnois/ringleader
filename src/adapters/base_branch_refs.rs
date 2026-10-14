use crate::domain::BaseBranch;

pub struct BaseBranchRefs<'a> {
    branch: &'a BaseBranch,
}

impl<'a> BaseBranchRefs<'a> {
    const LOCAL_BRANCH_PREFIX: &'static str = "refs/heads/";
    const REMOTE_TRACKING_PREFIX: &'static str = "refs/remotes/";
    const ORIGIN_REMOTE: &'static str = "origin";
    const ORIGIN_PREFIX: &'static str = "origin/";

    pub fn new(branch: &'a BaseBranch) -> Self {
        Self { branch }
    }

    pub fn local_ref(&self) -> String {
        format!("{}{}", Self::LOCAL_BRANCH_PREFIX, self.branch)
    }

    pub fn remote_tracking_ref(&self) -> String {
        format!("{}{}", Self::REMOTE_TRACKING_PREFIX, self.branch)
    }

    pub fn candidate_refs(&self) -> [String; 2] {
        [self.local_ref(), self.remote_tracking_ref()]
    }

    pub fn origin_suggestion(&self) -> String {
        format!("{}/{}", Self::ORIGIN_REMOTE, self.branch)
    }

    pub fn origin_ref(&self) -> String {
        format!(
            "{}{}",
            Self::REMOTE_TRACKING_PREFIX,
            self.origin_suggestion()
        )
    }

    pub fn on_origin(&self) -> Option<BaseBranch> {
        let name = self.branch.as_str();
        BaseBranch::new(name.strip_prefix(Self::ORIGIN_PREFIX).unwrap_or(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RefsFixture;

    impl RefsFixture {
        fn branch(name: &str) -> BaseBranch {
            BaseBranch::new(name).unwrap()
        }
    }

    #[test]
    fn names_a_local_branch_ref_rather_than_the_bare_name() {
        let branch = RefsFixture::branch("main");

        assert_eq!(BaseBranchRefs::new(&branch).local_ref(), "refs/heads/main");
    }

    #[test]
    fn names_a_remote_tracking_ref() {
        let branch = RefsFixture::branch("origin/main");

        assert_eq!(
            BaseBranchRefs::new(&branch).remote_tracking_ref(),
            "refs/remotes/origin/main"
        );
    }

    #[test]
    fn tries_the_local_branch_before_the_remote_tracking_one() {
        let branch = RefsFixture::branch("develop");

        assert_eq!(
            BaseBranchRefs::new(&branch).candidate_refs(),
            ["refs/heads/develop", "refs/remotes/develop"]
        );
    }

    #[test]
    fn suggests_the_origin_copy() {
        let branch = RefsFixture::branch("develop");

        assert_eq!(
            BaseBranchRefs::new(&branch).origin_suggestion(),
            "origin/develop"
        );
    }

    #[test]
    fn a_local_name_keeps_its_name_on_origin() {
        let branch = RefsFixture::branch("develop");

        assert_eq!(
            BaseBranchRefs::new(&branch).on_origin(),
            BaseBranch::new("develop")
        );
    }

    #[test]
    fn an_origin_tracking_name_drops_the_remote_on_origin() {
        let branch = RefsFixture::branch("origin/main");

        assert_eq!(
            BaseBranchRefs::new(&branch).on_origin(),
            BaseBranch::new("main")
        );
    }

    #[test]
    fn the_bare_origin_prefix_names_no_branch_on_origin() {
        let branch = RefsFixture::branch("origin/");

        assert_eq!(BaseBranchRefs::new(&branch).on_origin(), None);
    }

    #[test]
    fn names_the_ref_of_the_origin_copy() {
        let branch = RefsFixture::branch("develop");

        assert_eq!(
            BaseBranchRefs::new(&branch).origin_ref(),
            "refs/remotes/origin/develop"
        );
    }
}
