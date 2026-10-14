use std::path::Path;

use super::base_branch_ladders::BaseBranchLadders;
use super::ports::{StartPoint, Workspace};
use super::service_error::ServiceError;
use crate::domain::{BaseBranch, Branch};

pub struct StartPointResolver<'a> {
    workspace: &'a dyn Workspace,
    ladders: BaseBranchLadders<'a>,
}

impl<'a> StartPointResolver<'a> {
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
        repo: &Path,
        base_branch: Option<&BaseBranch>,
    ) -> Result<StartPoint, ServiceError> {
        match base_branch {
            Some(base) => self.resolve_configured(repo, base),
            None => self.resolve_ladder(repo),
        }
    }

    fn resolve_configured(
        &self,
        repo: &Path,
        base: &BaseBranch,
    ) -> Result<StartPoint, ServiceError> {
        let tips = self
            .workspace
            .branch_tips(repo, &base.branches_and_origin_copy())?;
        if let Some(commit) = tips.first_of(&base.branches()) {
            return Ok(StartPoint::Commit(commit.to_string()));
        }
        let origin_copy = base.origin_copy();
        let hint = tips
            .tip_of(&origin_copy)
            .map(|_| format!(", did you mean {}?", origin_copy.name()));
        Err(ServiceError::Invalid(format!(
            "base branch {base} matches no local or remote-tracking branch{}",
            hint.unwrap_or_default()
        )))
    }

    fn resolve_ladder(&self, repo: &Path) -> Result<StartPoint, ServiceError> {
        let candidates = self.ladders.at(repo)?.start_candidates();
        let tips = self.workspace.branch_tips(repo, &candidates)?;
        if let Some(commit) = tips.first_of(&candidates) {
            return Ok(StartPoint::Commit(commit.to_string()));
        }
        if self.workspace.head(repo)?.is_unborn() {
            return Ok(StartPoint::Orphan);
        }
        Err(ServiceError::Invalid(format!(
            "{} has no default branch on origin and no main or master branch, set base_branch for it",
            repo.display()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::FakeWorkspace;

    struct StartFixture;

    impl StartFixture {
        const REPO: &'static str = "/repos/example-project";
        const LOCAL_TIP: &'static str = "1111111111111111111111111111111111111111";
        const TRACKED_TIP: &'static str = "2222222222222222222222222222222222222222";
        const ORIGIN_TIP: &'static str = "3333333333333333333333333333333333333333";
        const MAIN_TIP: &'static str = "4444444444444444444444444444444444444444";

        fn repo() -> &'static Path {
            Path::new(Self::REPO)
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
                .with_branch_tips(Self::repo(), tips)
        }

        fn resolve(
            workspace: &FakeWorkspace,
            base_branch: Option<&str>,
        ) -> Result<StartPoint, ServiceError> {
            let base_branch = base_branch.and_then(BaseBranch::new);
            StartPointResolver::new(workspace).resolve(Self::repo(), base_branch.as_ref())
        }

        fn refusal(workspace: &FakeWorkspace, base_branch: Option<&str>) -> String {
            Self::resolve(workspace, base_branch)
                .unwrap_err()
                .to_string()
        }

        fn commit(tip: &str) -> StartPoint {
            StartPoint::Commit(tip.to_string())
        }
    }

    #[test]
    fn a_configured_local_branch_wins_over_its_remote_tracking_namesake() {
        let workspace = StartFixture::with_tips(&[
            (StartFixture::tracked("develop"), StartFixture::TRACKED_TIP),
            (StartFixture::local("develop"), StartFixture::LOCAL_TIP),
        ]);

        assert_eq!(
            StartFixture::resolve(&workspace, Some("develop")).unwrap(),
            StartFixture::commit(StartFixture::LOCAL_TIP)
        );
    }

    #[test]
    fn a_configured_remote_tracking_name_resolves() {
        let workspace = StartFixture::with_tips(&[(
            StartFixture::tracked("origin/main"),
            StartFixture::TRACKED_TIP,
        )]);

        assert_eq!(
            StartFixture::resolve(&workspace, Some("origin/main")).unwrap(),
            StartFixture::commit(StartFixture::TRACKED_TIP)
        );
    }

    #[test]
    fn a_configured_branch_only_on_origin_is_refused_with_a_hint() {
        let workspace = StartFixture::with_tips(&[(
            StartFixture::tracked("origin/develop"),
            StartFixture::ORIGIN_TIP,
        )]);

        let refusal = StartFixture::refusal(&workspace, Some("develop"));

        assert!(refusal.contains("did you mean origin/develop?"));
        assert!(!refusal.contains("refs/"));
    }

    #[test]
    fn a_configured_branch_found_nowhere_is_refused_without_a_hint() {
        let workspace = StartFixture::with_tips(&[]);

        let refusal = StartFixture::refusal(&workspace, Some("develop"));

        assert!(refusal.contains("base branch develop matches no local or remote-tracking branch"));
        assert!(!refusal.contains("did you mean"));
    }

    #[test]
    fn a_missing_configured_branch_never_falls_back_to_the_ladder() {
        let workspace =
            StartFixture::with_tips(&[(StartFixture::local("main"), StartFixture::MAIN_TIP)]);

        StartFixture::resolve(&workspace, Some("develop")).unwrap_err();

        assert!(
            !workspace
                .calls()
                .iter()
                .any(|call| call.starts_with("remote_default_branch"))
        );
    }

    #[test]
    fn the_origin_default_starts_from_its_local_branch() {
        let workspace = StartFixture::with_tips(&[
            (
                StartFixture::tracked("origin/trunk"),
                StartFixture::ORIGIN_TIP,
            ),
            (StartFixture::local("trunk"), StartFixture::LOCAL_TIP),
            (StartFixture::local("main"), StartFixture::MAIN_TIP),
        ])
        .with_remote_default(StartFixture::repo(), "trunk");

        assert_eq!(
            StartFixture::resolve(&workspace, None).unwrap(),
            StartFixture::commit(StartFixture::LOCAL_TIP)
        );
    }

    #[test]
    fn the_origin_default_falls_back_to_origin_without_a_local_branch() {
        let workspace = StartFixture::with_tips(&[
            (
                StartFixture::tracked("origin/trunk"),
                StartFixture::ORIGIN_TIP,
            ),
            (StartFixture::local("main"), StartFixture::MAIN_TIP),
        ])
        .with_remote_default(StartFixture::repo(), "trunk");

        assert_eq!(
            StartFixture::resolve(&workspace, None).unwrap(),
            StartFixture::commit(StartFixture::ORIGIN_TIP)
        );
    }

    #[test]
    fn without_an_origin_default_main_comes_before_master() {
        let workspace = StartFixture::with_tips(&[
            (StartFixture::local("master"), StartFixture::LOCAL_TIP),
            (StartFixture::local("main"), StartFixture::MAIN_TIP),
        ]);

        assert_eq!(
            StartFixture::resolve(&workspace, None).unwrap(),
            StartFixture::commit(StartFixture::MAIN_TIP)
        );
    }

    #[test]
    fn a_session_branch_named_main_leaves_master_to_start_from() {
        let workspace = StartFixture::with_tips(&[
            (StartFixture::local("main"), StartFixture::MAIN_TIP),
            (StartFixture::local("master"), StartFixture::LOCAL_TIP),
        ]);
        let resolver =
            StartPointResolver::new(&workspace).skipping(vec![StartFixture::local("main")]);

        assert_eq!(
            resolver.resolve(StartFixture::repo(), None).unwrap(),
            StartFixture::commit(StartFixture::LOCAL_TIP)
        );
    }

    #[test]
    fn origin_main_alone_is_no_start_point_without_an_origin_default() {
        let workspace = StartFixture::with_tips(&[(
            StartFixture::tracked("origin/main"),
            StartFixture::ORIGIN_TIP,
        )]);

        assert!(StartFixture::refusal(&workspace, None).contains("set base_branch for it"));
    }

    #[test]
    fn an_unborn_repo_without_branches_starts_an_orphan() {
        let workspace = FakeWorkspace::default().starting_orphan();

        assert_eq!(
            StartFixture::resolve(&workspace, None).unwrap(),
            StartPoint::Orphan
        );
    }

    #[test]
    fn a_repo_with_history_but_no_base_branch_is_refused_by_name() {
        let workspace = StartFixture::with_tips(&[]);

        let refusal = StartFixture::refusal(&workspace, None);

        assert!(refusal.starts_with(StartFixture::REPO));
        assert!(refusal.contains("no default branch on origin and no main or master branch"));
    }
}
