use std::path::Path;

use super::base_branch_ladders::BaseBranchLadders;
use super::ports::Workspace;
use super::service_error::ServiceError;
use crate::domain::{BaseBranch, Branch};

pub struct UpstreamResolver<'a> {
    workspace: &'a dyn Workspace,
    ladders: BaseBranchLadders<'a>,
}

impl<'a> UpstreamResolver<'a> {
    pub fn new(workspace: &'a dyn Workspace) -> Self {
        Self {
            workspace,
            ladders: BaseBranchLadders::new(workspace),
        }
    }

    pub fn skipping(self, session_branches: Vec<Branch>) -> Self {
        Self {
            ladders: self.ladders.skipping(session_branches),
            ..self
        }
    }

    pub fn resolve(
        &self,
        worktree: &Path,
        base_branch: Option<&BaseBranch>,
    ) -> Result<BaseBranch, ServiceError> {
        match base_branch {
            Some(base) => Self::on_origin(base),
            None => self.origin_default(worktree),
        }
    }

    fn on_origin(base: &BaseBranch) -> Result<BaseBranch, ServiceError> {
        base.on_origin().ok_or_else(|| {
            ServiceError::Invalid(format!("base branch {base} names no branch on origin"))
        })
    }

    fn origin_default(&self, worktree: &Path) -> Result<BaseBranch, ServiceError> {
        let ladder = self.ladders.at(worktree)?;
        let tips = self
            .workspace
            .branch_tips(worktree, &ladder.upstream_lookups())?;
        ladder.upstream(&tips).ok_or_else(|| {
            ServiceError::Invalid(format!(
                "{} has no default branch, main, or master on origin, add an origin remote or set base_branch for its repo",
                worktree.display()
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::FakeWorkspace;

    struct UpstreamFixture;

    impl UpstreamFixture {
        const WORKTREE: &'static str = "/worktrees/fix-auth";
        const COMMIT: &'static str = "1111111111111111111111111111111111111111";

        fn worktree() -> &'static Path {
            Path::new(Self::WORKTREE)
        }

        fn on_origin(names: &[&str]) -> FakeWorkspace {
            let origin_tips: Vec<(Branch, &str)> = names
                .iter()
                .map(|name| (Branch::origin_copy_of(name), Self::COMMIT))
                .collect();
            FakeWorkspace::default()
                .without_branch_tips()
                .with_branch_tips(Self::worktree(), &origin_tips)
        }

        fn resolve(
            workspace: &FakeWorkspace,
            base_branch: Option<&str>,
        ) -> Result<BaseBranch, ServiceError> {
            let base_branch = base_branch.and_then(BaseBranch::new);
            UpstreamResolver::new(workspace).resolve(Self::worktree(), base_branch.as_ref())
        }

        fn branch(name: &str) -> BaseBranch {
            BaseBranch::new(name).unwrap()
        }
    }

    #[test]
    fn a_base_branch_is_pulled_by_its_name_on_origin() {
        let workspace = UpstreamFixture::on_origin(&["main"]);

        assert_eq!(
            UpstreamFixture::resolve(&workspace, Some("origin/develop")).unwrap(),
            UpstreamFixture::branch("develop")
        );
    }

    #[test]
    fn a_base_branch_naming_no_branch_on_origin_is_refused() {
        let workspace = UpstreamFixture::on_origin(&["main"]);

        assert_eq!(
            UpstreamFixture::resolve(&workspace, Some("origin/")).unwrap_err(),
            ServiceError::Invalid("base branch origin/ names no branch on origin".to_string())
        );
    }

    #[test]
    fn the_origin_default_is_pulled_over_main() {
        let workspace = UpstreamFixture::on_origin(&["main", "trunk"])
            .with_remote_default(UpstreamFixture::worktree(), "trunk");

        assert_eq!(
            UpstreamFixture::resolve(&workspace, None).unwrap(),
            UpstreamFixture::branch("trunk")
        );
    }

    #[test]
    fn without_an_origin_default_main_is_pulled_before_master() {
        let workspace = UpstreamFixture::on_origin(&["master", "main"]);

        assert_eq!(
            UpstreamFixture::resolve(&workspace, None).unwrap(),
            UpstreamFixture::branch("main")
        );
    }

    #[test]
    fn a_session_branch_named_main_is_never_pulled() {
        let workspace = UpstreamFixture::on_origin(&["main", "master"]);
        let resolver =
            UpstreamResolver::new(&workspace).skipping(vec![Branch::Local("main".to_string())]);

        assert_eq!(
            resolver.resolve(UpstreamFixture::worktree(), None).unwrap(),
            UpstreamFixture::branch("master")
        );
    }

    #[test]
    fn a_worktree_with_nothing_to_pull_is_refused_by_path() {
        let workspace = UpstreamFixture::on_origin(&[]);

        assert_eq!(
            UpstreamFixture::resolve(&workspace, None).unwrap_err(),
            ServiceError::Invalid(format!(
                "{} has no default branch, main, or master on origin, add an origin remote or set base_branch for its repo",
                UpstreamFixture::WORKTREE
            ))
        );
    }
}
