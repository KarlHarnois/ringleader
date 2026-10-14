use std::iter;
use std::path::Path;

use super::base_branch_ladders::BaseBranchLadders;
use super::ports::{StartPoint, Workspace, WorkspaceError};
use crate::domain::{BaseBranch, Branch, BranchTips};

pub struct DiffBaseResolver<'a> {
    workspace: &'a dyn Workspace,
    ladders: BaseBranchLadders<'a>,
}

impl<'a> DiffBaseResolver<'a> {
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
        recorded: &StartPoint,
        base_branch: Option<&BaseBranch>,
    ) -> Result<StartPoint, WorkspaceError> {
        match recorded {
            StartPoint::Commit(commit) => self
                .measured_from(worktree, commit, base_branch)
                .map(StartPoint::Commit),
            StartPoint::Orphan => Ok(StartPoint::Orphan),
        }
    }

    fn measured_from(
        &self,
        worktree: &Path,
        recorded: &str,
        base_branch: Option<&BaseBranch>,
    ) -> Result<String, WorkspaceError> {
        let head = self.workspace.head(worktree)?;
        if head.is_unborn() {
            return Ok(recorded.to_string());
        }
        let base_commits = self.base_branch_commits(worktree, base_branch, head.branch())?;
        let candidates: Vec<String> = iter::once(recorded.to_string())
            .chain(base_commits)
            .collect();
        let bases = self.workspace.merge_bases(worktree, &candidates)?;
        Ok(bases.measured_from(recorded).to_string())
    }

    fn base_branch_commits(
        &self,
        worktree: &Path,
        base_branch: Option<&BaseBranch>,
        checked_out: Option<&Branch>,
    ) -> Result<Vec<String>, WorkspaceError> {
        match base_branch {
            Some(base) => self.configured_commits(worktree, base, checked_out),
            None => self.ladder_commits(worktree, checked_out),
        }
    }

    fn configured_commits(
        &self,
        worktree: &Path,
        base: &BaseBranch,
        checked_out: Option<&Branch>,
    ) -> Result<Vec<String>, WorkspaceError> {
        let rung = base.branches_and_origin_copy();
        let tips = self.workspace.branch_tips(worktree, &rung)?;
        Ok(Self::commits_besides(&tips, rung, checked_out))
    }

    fn ladder_commits(
        &self,
        worktree: &Path,
        checked_out: Option<&Branch>,
    ) -> Result<Vec<String>, WorkspaceError> {
        let ladder = self.ladders.at(worktree)?;
        let tips = self
            .workspace
            .branch_tips(worktree, &ladder.diff_lookups())?;
        let rung = ladder.diff_rung(&tips, checked_out);
        Ok(Self::commits_besides(&tips, rung, checked_out))
    }

    fn commits_besides(
        tips: &BranchTips,
        mut rung: Vec<Branch>,
        checked_out: Option<&Branch>,
    ) -> Vec<String> {
        rung.retain(|branch| Some(branch) != checked_out);
        tips.commits_of(&rung)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::Head;
    use crate::test_support::FakeWorkspace;

    struct DiffFixture;

    impl DiffFixture {
        const WORKTREE: &'static str = "/worktrees/fix-auth";
        const RECORDED: &'static str = "1111111111111111111111111111111111111111";
        const UPSTREAM: &'static str = "2222222222222222222222222222222222222222";
        const MAIN_TIP: &'static str = "3333333333333333333333333333333333333333";
        const ORIGIN_TIP: &'static str = "4444444444444444444444444444444444444444";
        const MASTER_TIP: &'static str = "5555555555555555555555555555555555555555";

        fn worktree() -> &'static Path {
            Path::new(Self::WORKTREE)
        }

        fn local(name: &str) -> Branch {
            Branch::Local(name.to_string())
        }

        fn tracked(name: &str) -> Branch {
            Branch::RemoteTracking(name.to_string())
        }

        fn with_tips(tips: &[(Branch, &str)]) -> FakeWorkspace {
            FakeWorkspace::default()
                .without_branch_tips()
                .with_branch_tips(Self::worktree(), tips)
        }

        fn on(workspace: FakeWorkspace, branch: Branch) -> FakeWorkspace {
            workspace.with_head(Self::worktree(), Head::On(branch))
        }

        fn resolve(workspace: &FakeWorkspace, base_branch: Option<&str>) -> StartPoint {
            Self::resolve_from(
                workspace,
                &StartPoint::Commit(Self::RECORDED.to_string()),
                base_branch,
            )
        }

        fn resolve_from(
            workspace: &FakeWorkspace,
            recorded: &StartPoint,
            base_branch: Option<&str>,
        ) -> StartPoint {
            let base_branch = base_branch.and_then(BaseBranch::new);
            DiffBaseResolver::new(workspace)
                .resolve(Self::worktree(), recorded, base_branch.as_ref())
                .unwrap()
        }

        fn asked_about(workspace: &FakeWorkspace, base_branch: Option<&str>) -> Vec<Vec<String>> {
            Self::resolve(workspace, base_branch);
            workspace.merge_base_requests()
        }

        fn recorded_and(tips: &[&str]) -> Vec<Vec<String>> {
            let commits = iter::once(Self::RECORDED).chain(tips.iter().copied());
            vec![commits.map(str::to_string).collect()]
        }
    }

    #[test]
    fn an_orphan_session_measures_from_nothing_without_asking_git() {
        let workspace = FakeWorkspace::default();

        let base = DiffFixture::resolve_from(&workspace, &StartPoint::Orphan, None);

        assert_eq!(base, StartPoint::Orphan);
        assert!(workspace.calls().is_empty());
    }

    #[test]
    fn an_unborn_head_measures_from_the_recorded_commit() {
        let workspace = FakeWorkspace::default().with_head(DiffFixture::worktree(), Head::Unborn);

        let base = DiffFixture::resolve(&workspace, None);

        assert_eq!(base, StartPoint::Commit(DiffFixture::RECORDED.to_string()));
        assert!(workspace.merge_base_requests().is_empty());
    }

    #[test]
    fn the_recorded_commit_leads_the_base_branch_tips_it_is_measured_with() {
        let workspace = DiffFixture::with_tips(&[
            (DiffFixture::tracked("origin/main"), DiffFixture::ORIGIN_TIP),
            (DiffFixture::local("main"), DiffFixture::MAIN_TIP),
        ]);

        assert_eq!(
            DiffFixture::asked_about(&workspace, None),
            DiffFixture::recorded_and(&[DiffFixture::MAIN_TIP, DiffFixture::ORIGIN_TIP])
        );
    }

    #[test]
    fn without_a_base_branch_tip_the_recorded_commit_is_still_measured() {
        let workspace = DiffFixture::with_tips(&[]);

        assert_eq!(
            DiffFixture::asked_about(&workspace, None),
            DiffFixture::recorded_and(&[])
        );
    }

    #[test]
    fn without_a_merge_base_the_diff_measures_from_the_recorded_commit() {
        let workspace = DiffFixture::with_tips(&[]);

        assert_eq!(
            DiffFixture::resolve(&workspace, None),
            StartPoint::Commit(DiffFixture::RECORDED.to_string())
        );
    }

    #[test]
    fn a_tie_measures_from_the_base_reached_through_the_base_branch() {
        let workspace = DiffFixture::with_tips(&[]).with_merge_bases(
            DiffFixture::worktree(),
            &[DiffFixture::RECORDED, DiffFixture::UPSTREAM],
        );

        assert_eq!(
            DiffFixture::resolve(&workspace, None),
            StartPoint::Commit(DiffFixture::UPSTREAM.to_string())
        );
    }

    #[test]
    fn a_configured_base_branch_is_looked_up_with_its_origin_copy_and_never_the_ladder() {
        let workspace = DiffFixture::with_tips(&[]);

        DiffFixture::resolve(&workspace, Some("develop"));

        let develop = BaseBranch::new("develop").unwrap();
        let lookups: Vec<Vec<Branch>> = workspace
            .branch_tip_requests()
            .into_iter()
            .map(|(_, branches)| branches)
            .collect();
        assert_eq!(lookups, [develop.branches_and_origin_copy()]);
        assert!(
            !workspace
                .calls()
                .iter()
                .any(|call| call.starts_with("remote_default_branch"))
        );
    }

    #[test]
    fn a_checked_out_configured_branch_is_left_out() {
        let workspace = DiffFixture::on(
            DiffFixture::with_tips(&[
                (DiffFixture::local("develop"), DiffFixture::MAIN_TIP),
                (
                    DiffFixture::tracked("origin/develop"),
                    DiffFixture::ORIGIN_TIP,
                ),
            ]),
            DiffFixture::local("develop"),
        );

        assert_eq!(
            DiffFixture::asked_about(&workspace, Some("develop")),
            DiffFixture::recorded_and(&[DiffFixture::ORIGIN_TIP])
        );
    }

    #[test]
    fn the_origin_default_pair_is_measured_with_whatever_main_holds() {
        let workspace = DiffFixture::with_tips(&[
            (DiffFixture::local("trunk"), DiffFixture::MASTER_TIP),
            (
                DiffFixture::tracked("origin/trunk"),
                DiffFixture::ORIGIN_TIP,
            ),
            (DiffFixture::local("main"), DiffFixture::MAIN_TIP),
        ])
        .with_remote_default(DiffFixture::worktree(), "trunk");

        assert_eq!(
            DiffFixture::asked_about(&workspace, None),
            DiffFixture::recorded_and(&[DiffFixture::MASTER_TIP, DiffFixture::ORIGIN_TIP])
        );
    }

    #[test]
    fn a_checked_out_origin_default_is_left_out() {
        let workspace = DiffFixture::on(
            DiffFixture::with_tips(&[
                (DiffFixture::local("trunk"), DiffFixture::MASTER_TIP),
                (
                    DiffFixture::tracked("origin/trunk"),
                    DiffFixture::ORIGIN_TIP,
                ),
            ])
            .with_remote_default(DiffFixture::worktree(), "trunk"),
            DiffFixture::local("trunk"),
        );

        assert_eq!(
            DiffFixture::asked_about(&workspace, None),
            DiffFixture::recorded_and(&[DiffFixture::ORIGIN_TIP])
        );
    }

    #[test]
    fn without_an_origin_default_main_and_its_origin_copy_are_measured_with() {
        let workspace = DiffFixture::with_tips(&[
            (DiffFixture::local("master"), DiffFixture::MASTER_TIP),
            (DiffFixture::tracked("origin/main"), DiffFixture::ORIGIN_TIP),
            (DiffFixture::local("main"), DiffFixture::MAIN_TIP),
        ]);

        assert_eq!(
            DiffFixture::asked_about(&workspace, None),
            DiffFixture::recorded_and(&[DiffFixture::MAIN_TIP, DiffFixture::ORIGIN_TIP])
        );
    }

    #[test]
    fn a_session_on_main_is_measured_with_master_and_its_origin_copy() {
        let workspace = DiffFixture::on(
            DiffFixture::with_tips(&[
                (DiffFixture::local("main"), DiffFixture::MAIN_TIP),
                (DiffFixture::local("master"), DiffFixture::MASTER_TIP),
                (
                    DiffFixture::tracked("origin/master"),
                    DiffFixture::ORIGIN_TIP,
                ),
            ]),
            DiffFixture::local("main"),
        );

        assert_eq!(
            DiffFixture::asked_about(&workspace, None),
            DiffFixture::recorded_and(&[DiffFixture::MASTER_TIP, DiffFixture::ORIGIN_TIP])
        );
    }

    #[test]
    fn another_sessions_branch_named_main_leaves_master_to_measure_with() {
        let workspace = DiffFixture::with_tips(&[
            (DiffFixture::local("main"), DiffFixture::MAIN_TIP),
            (DiffFixture::local("master"), DiffFixture::MASTER_TIP),
        ]);
        let recorded = StartPoint::Commit(DiffFixture::RECORDED.to_string());

        DiffBaseResolver::new(&workspace)
            .skipping(vec![DiffFixture::local("main")])
            .resolve(DiffFixture::worktree(), &recorded, None)
            .unwrap();

        assert_eq!(
            workspace.merge_base_requests(),
            DiffFixture::recorded_and(&[DiffFixture::MASTER_TIP])
        );
    }
}
