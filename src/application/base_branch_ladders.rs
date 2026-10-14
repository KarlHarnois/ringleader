use std::path::Path;

use super::ports::{Workspace, WorkspaceError};
use crate::domain::{BaseBranchLadder, Branch};

pub struct BaseBranchLadders<'a> {
    workspace: &'a dyn Workspace,
    session_branches: Vec<Branch>,
}

impl<'a> BaseBranchLadders<'a> {
    pub fn new(workspace: &'a dyn Workspace) -> Self {
        Self {
            workspace,
            session_branches: Vec::new(),
        }
    }

    pub fn skipping(self, session_branches: Vec<Branch>) -> Self {
        Self {
            session_branches,
            ..self
        }
    }

    pub fn at(&self, location: &Path) -> Result<BaseBranchLadder, WorkspaceError> {
        let origin_default = self
            .workspace
            .remote_default_branch(location, Branch::ORIGIN)?;
        Ok(BaseBranchLadder::new(origin_default).skipping(&self.session_branches))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::FakeWorkspace;

    struct LaddersFixture;

    impl LaddersFixture {
        const REPO: &'static str = "/repos/example-project";

        fn repo() -> &'static Path {
            Path::new(Self::REPO)
        }

        fn local(name: &str) -> Branch {
            Branch::Local(name.to_string())
        }
    }

    #[test]
    fn the_ladder_leads_with_origins_default_at_the_location() {
        let workspace =
            FakeWorkspace::default().with_remote_default(LaddersFixture::repo(), "trunk");

        let ladder = BaseBranchLadders::new(&workspace)
            .at(LaddersFixture::repo())
            .unwrap();

        assert_eq!(ladder.start_candidates()[0], LaddersFixture::local("trunk"));
    }

    #[test]
    fn the_ladder_skips_the_session_branches() {
        let workspace = FakeWorkspace::default();

        let ladder = BaseBranchLadders::new(&workspace)
            .skipping(vec![LaddersFixture::local("main")])
            .at(LaddersFixture::repo())
            .unwrap();

        assert_eq!(ladder.start_candidates(), [LaddersFixture::local("master")]);
    }
}
