use std::path::Path;
use std::sync::Arc;

use super::checkout_strategy::CheckoutStrategy;
use super::creation_plan::CreationPlan;
use crate::application::ServiceError;
use crate::application::ServicePorts;
use crate::application::ports::{HostProbe, StartPoint, Workspace, WorktreeEntry};
use crate::application::reconciliation::Reconciliation;
use crate::domain::{Session, SessionWorktree};

pub struct MainCheckoutStrategy {
    workspace: Arc<dyn Workspace>,
    host: Arc<dyn HostProbe>,
}

impl MainCheckoutStrategy {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            workspace: ports.repos().workspace(),
            host: ports.repos().host(),
        }
    }

    fn refuse_base_branch(plan: &CreationPlan) -> Result<(), ServiceError> {
        if plan.requested_base_branch().is_none() {
            return Ok(());
        }
        Err(ServiceError::Invalid(
            "a session in the main checkout runs on the branch checked out there, so it takes no base branch"
                .to_string(),
        ))
    }

    fn refuse_second_session(
        &self,
        plan: &CreationPlan,
        reconciliation: &Reconciliation,
    ) -> Result<(), ServiceError> {
        let repo = self.host.canonical_or_given(plan.repo_path());
        match reconciliation
            .entries()
            .iter()
            .find(|entry| self.runs_in(entry, &repo))
        {
            Some(entry) => Err(ServiceError::Conflict(format!(
                "session {} already runs in the main checkout {}, create this session in a new worktree",
                entry.name(),
                plan.repo_path().display()
            ))),
            None => Ok(()),
        }
    }

    fn runs_in(&self, entry: &Session, checkout: &Path) -> bool {
        self.host.canonical_or_given(entry.worktree_path()) == checkout
    }

    fn main_entry(&self, plan: &CreationPlan) -> Result<WorktreeEntry, ServiceError> {
        let canonical = self.host.canonicalize_lenient(plan.repo_path())?;
        self.workspace
            .list_worktrees(plan.repo_path())?
            .into_iter()
            .find(|entry| entry.is_main() && entry.path() == canonical)
            .ok_or_else(|| {
                ServiceError::Invalid(format!(
                    "{} is not the main checkout of its repository",
                    plan.repo_path().display()
                ))
            })
    }

    fn checked_out_branch(entry: &WorktreeEntry) -> Result<&str, ServiceError> {
        entry.branch().ok_or_else(|| {
            ServiceError::Conflict(format!(
                "the main checkout {} has a detached HEAD, check out a branch there before creating a session in it",
                entry.path().display()
            ))
        })
    }

    fn start_of(entry: &WorktreeEntry) -> StartPoint {
        entry.head().map_or(StartPoint::Orphan, |head| {
            StartPoint::Commit(head.to_string())
        })
    }
}

impl CheckoutStrategy for MainCheckoutStrategy {
    fn prepare(
        &self,
        plan: &CreationPlan,
        reconciliation: &Reconciliation,
    ) -> Result<SessionWorktree, ServiceError> {
        Self::refuse_base_branch(plan)?;
        self.refuse_second_session(plan, reconciliation)?;
        let entry = self.main_entry(plan)?;
        let branch = Self::checked_out_branch(&entry)?;
        Ok(plan.main_checkout_on(branch, &Self::start_of(&entry)))
    }

    fn undo(
        &self,
        _plan: &CreationPlan,
        _worktree: &SessionWorktree,
        error: ServiceError,
    ) -> ServiceError {
        error
    }

    fn report_kept(&self, _worktree: &SessionWorktree, error: ServiceError) -> ServiceError {
        error
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::MultiplexerError;
    use crate::application::start::creation_plan::CreateRequest;
    use crate::domain::{BaseBranch, Checkout};
    use crate::test_support::{FakeMultiplexer, FakeWorkspace, ServiceFixture};

    struct MainCheckoutFixture;

    impl MainCheckoutFixture {
        fn request(name: &str) -> CreateRequest {
            CreateRequest::new(name, ServiceFixture::repo()).with_checkout(Checkout::Main)
        }

        fn fixture() -> ServiceFixture {
            ServiceFixture::new().with_workspace(ServiceFixture::main_checkout_workspace())
        }

        fn with_main_entry(entry: WorktreeEntry) -> ServiceFixture {
            let workspace = FakeWorkspace::default().with_worktree(Self::repo(), entry.main());
            ServiceFixture::new().with_workspace(workspace)
        }

        fn repo() -> &'static Path {
            Path::new(ServiceFixture::REPO)
        }

        fn create(fixture: &ServiceFixture) -> Result<Session, ServiceError> {
            Self::create_from(fixture, Self::request("fix-auth"))
        }

        fn create_from(
            fixture: &ServiceFixture,
            request: CreateRequest,
        ) -> Result<Session, ServiceError> {
            fixture
                .service()
                .create_session(&request)
                .map(|outcome| outcome.session().clone())
        }

        fn failing_tmux(multiplexer: FakeMultiplexer) -> ServiceFixture {
            Self::fixture().with_multiplexer(multiplexer)
        }

        fn tmux_timeout() -> MultiplexerError {
            MultiplexerError::TimedOut {
                subcommand: "new-session".to_string(),
                seconds: 10,
            }
        }
    }

    #[test]
    fn a_session_in_the_main_checkout_records_the_repo_as_its_worktree() {
        let session = MainCheckoutFixture::create(&MainCheckoutFixture::fixture()).unwrap();

        assert_eq!(session.worktree_path(), MainCheckoutFixture::repo());
    }

    #[test]
    fn a_session_in_the_main_checkout_records_the_branch_checked_out_there() {
        let session = MainCheckoutFixture::create(&MainCheckoutFixture::fixture()).unwrap();

        assert_eq!(session.branch(), "main");
    }

    #[test]
    fn a_session_in_the_main_checkout_starts_from_the_checked_out_commit() {
        let session = MainCheckoutFixture::create(&MainCheckoutFixture::fixture()).unwrap();

        assert_eq!(session.base_commit(), ServiceFixture::TIP);
    }

    #[test]
    fn a_session_in_an_unborn_main_checkout_starts_from_an_orphan() {
        let fixture = MainCheckoutFixture::with_main_entry(
            WorktreeEntry::new(MainCheckoutFixture::repo()).on_branch("main"),
        );

        let session = MainCheckoutFixture::create(&fixture).unwrap();

        assert_eq!(session.base_commit(), StartPoint::Orphan.base_commit());
    }

    #[test]
    fn a_session_in_the_main_checkout_changes_nothing_in_git() {
        let fixture = MainCheckoutFixture::fixture();

        MainCheckoutFixture::create(&fixture).unwrap();

        assert!(fixture.workspace().state_changing_calls().is_empty());
    }

    #[test]
    fn a_session_in_the_main_checkout_launches_its_agent_in_the_repo() {
        let fixture = MainCheckoutFixture::fixture();

        MainCheckoutFixture::create(&fixture).unwrap();

        let launch = fixture.multiplexer().launches().pop().unwrap();
        assert_eq!(launch.worktree(), MainCheckoutFixture::repo());
    }

    #[test]
    fn a_session_in_the_main_checkout_may_share_its_name_with_the_checked_out_branch() {
        let fixture = MainCheckoutFixture::fixture();

        let created =
            MainCheckoutFixture::create_from(&fixture, MainCheckoutFixture::request("main"));

        assert!(created.is_ok());
    }

    #[test]
    fn a_main_checkout_on_a_detached_head_is_refused() {
        let fixture = MainCheckoutFixture::with_main_entry(
            WorktreeEntry::new(MainCheckoutFixture::repo()).at_head(ServiceFixture::TIP),
        );

        let error = MainCheckoutFixture::create(&fixture).unwrap_err();

        assert!(matches!(error, ServiceError::Conflict(_)));
        assert!(error.to_string().contains("detached HEAD"));
    }

    #[test]
    fn a_second_session_in_the_same_main_checkout_is_refused() {
        let first = ServiceFixture::main_checkout_session("first");
        let fixture = MainCheckoutFixture::fixture().with_sessions(&[first]);

        let error = MainCheckoutFixture::create(&fixture).unwrap_err();

        assert_eq!(
            error,
            ServiceError::Conflict(format!(
                "session first already runs in the main checkout {}, create this session in a new worktree",
                ServiceFixture::REPO
            ))
        );
    }

    #[test]
    fn a_session_in_the_main_checkout_takes_no_base_branch() {
        let request = MainCheckoutFixture::request("fix-auth")
            .with_base_branch(BaseBranch::new("develop").unwrap());

        let error =
            MainCheckoutFixture::create_from(&MainCheckoutFixture::fixture(), request).unwrap_err();

        assert!(matches!(error, ServiceError::Invalid(_)));
    }

    #[test]
    fn a_repo_path_git_does_not_list_as_the_main_checkout_is_refused() {
        let error = MainCheckoutFixture::create(&ServiceFixture::new()).unwrap_err();

        assert_eq!(
            error,
            ServiceError::Invalid(format!(
                "{} is not the main checkout of its repository",
                ServiceFixture::REPO
            ))
        );
    }

    #[test]
    fn a_repo_path_git_lists_as_a_linked_worktree_is_refused() {
        let elsewhere = Path::new("/repos/example-project-main");
        let workspace = FakeWorkspace::default()
            .with_worktree(
                MainCheckoutFixture::repo(),
                WorktreeEntry::new(elsewhere).on_branch("main").main(),
            )
            .with_worktree(
                MainCheckoutFixture::repo(),
                WorktreeEntry::new(MainCheckoutFixture::repo()).on_branch("feature"),
            );
        let fixture = ServiceFixture::new().with_workspace(workspace);

        let error = MainCheckoutFixture::create(&fixture).unwrap_err();

        assert!(matches!(error, ServiceError::Invalid(_)));
    }

    #[test]
    fn a_failed_tmux_creation_in_the_main_checkout_discards_only_the_scratch_directory() {
        let failure = MultiplexerError::Failed {
            subcommand: "new-session".to_string(),
            detail: "duplicate session".to_string(),
        };
        let fixture = MainCheckoutFixture::failing_tmux(
            FakeMultiplexer::new().failing("create_session", failure),
        );

        MainCheckoutFixture::create(&fixture).unwrap_err();

        assert!(fixture.workspace().state_changing_calls().is_empty());
        assert_eq!(
            fixture.scratch_space().removed(),
            [MainCheckoutFixture::repo()]
        );
    }

    #[test]
    fn a_timed_out_tmux_creation_in_the_main_checkout_kills_the_session_and_changes_nothing_in_git()
    {
        let fixture = MainCheckoutFixture::failing_tmux(
            FakeMultiplexer::new()
                .failing("create_session", MainCheckoutFixture::tmux_timeout())
                .creating_despite_failure(),
        );

        MainCheckoutFixture::create(&fixture).unwrap_err();

        assert!(fixture.workspace().state_changing_calls().is_empty());
        assert!(fixture.multiplexer().live_names().is_empty());
    }
}
