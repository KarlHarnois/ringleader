use std::path::Path;

use super::ports::{Workspace, WorkspaceError};
use crate::domain::BaseBranch;

pub struct BaseBranchResolver<'a> {
    workspace: &'a dyn Workspace,
}

impl<'a> BaseBranchResolver<'a> {
    pub fn new(workspace: &'a dyn Workspace) -> Self {
        Self { workspace }
    }

    pub fn resolve<'b>(
        &self,
        worktree: &Path,
        requested: Option<&'b BaseBranch>,
        configured: Option<&'b BaseBranch>,
    ) -> Result<Option<&'b BaseBranch>, WorkspaceError> {
        let Some(requested) = requested else {
            return Ok(configured);
        };
        if self.still_exists(worktree, requested)? {
            return Ok(Some(requested));
        }
        Ok(configured)
    }

    fn still_exists(&self, worktree: &Path, branch: &BaseBranch) -> Result<bool, WorkspaceError> {
        let lookups = branch.branches_and_origin_copy();
        let tips = self.workspace.branch_tips(worktree, &lookups)?;
        Ok(tips.first_of(&lookups).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Branch;
    use crate::test_support::FakeWorkspace;

    struct ResolverFixture;

    impl ResolverFixture {
        const WORKTREE: &'static str = "/worktrees/part2";
        const COMMIT: &'static str = "1111111111111111111111111111111111111111";

        fn worktree() -> &'static Path {
            Path::new(Self::WORKTREE)
        }

        fn branch(name: &str) -> BaseBranch {
            BaseBranch::new(name).unwrap()
        }

        fn with_tip_of(branch: Branch) -> FakeWorkspace {
            FakeWorkspace::default()
                .without_branch_tips()
                .with_branch_tips(Self::worktree(), &[(branch, Self::COMMIT)])
        }

        fn resolve(
            workspace: &FakeWorkspace,
            requested: &BaseBranch,
            configured: &BaseBranch,
        ) -> Option<BaseBranch> {
            BaseBranchResolver::new(workspace)
                .resolve(Self::worktree(), Some(requested), Some(configured))
                .unwrap()
                .cloned()
        }
    }

    #[test]
    fn a_requested_branch_with_a_local_branch_is_followed() {
        let part1 = ResolverFixture::branch("part1");
        let workspace = ResolverFixture::with_tip_of(Branch::Local("part1".to_string()));

        assert_eq!(
            ResolverFixture::resolve(&workspace, &part1, &ResolverFixture::branch("develop")),
            Some(part1)
        );
    }

    #[test]
    fn a_requested_branch_left_only_on_origin_is_followed() {
        let part1 = ResolverFixture::branch("part1");
        let workspace = ResolverFixture::with_tip_of(Branch::origin_copy_of("part1"));

        assert_eq!(
            ResolverFixture::resolve(&workspace, &part1, &ResolverFixture::branch("develop")),
            Some(part1)
        );
    }

    #[test]
    fn a_requested_branch_that_no_longer_exists_gives_way_to_the_configured_one() {
        let develop = ResolverFixture::branch("develop");
        let workspace = ResolverFixture::with_tip_of(Branch::Local("main".to_string()));

        assert_eq!(
            ResolverFixture::resolve(&workspace, &ResolverFixture::branch("part1"), &develop),
            Some(develop)
        );
    }

    #[test]
    fn a_requested_branch_that_no_longer_exists_without_a_configured_one_leaves_the_ladder() {
        let part1 = ResolverFixture::branch("part1");
        let workspace = ResolverFixture::with_tip_of(Branch::Local("main".to_string()));

        let resolved = BaseBranchResolver::new(&workspace)
            .resolve(ResolverFixture::worktree(), Some(&part1), None)
            .unwrap();

        assert_eq!(resolved, None);
    }

    #[test]
    fn without_a_requested_branch_the_configured_one_is_followed_without_a_lookup() {
        let develop = ResolverFixture::branch("develop");
        let workspace = FakeWorkspace::default();

        let resolved = BaseBranchResolver::new(&workspace)
            .resolve(ResolverFixture::worktree(), None, Some(&develop))
            .unwrap();

        assert_eq!(resolved, Some(&develop));
        assert!(workspace.branch_tip_requests().is_empty());
    }
}
