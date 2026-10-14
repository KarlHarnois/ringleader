use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::agent_hooks::AgentHooks;
use super::branch_availability::BranchAvailability;
use super::creation_plan::{CreateRequest, CreationPlan};
use super::ports::{Clock, HostProbe, Multiplexer, SessionRegistry, StartPoint, Workspace};
use super::reconciliation::Reconciliation;
use super::repo_merge::RepoMerge;
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use super::session_scratch::SessionScratch;
use super::start_point_resolver::StartPointResolver;
use super::use_checks::UseChecks;
use super::worktree_rollback::WorktreeRollback;
use crate::domain::{Config, Session};

pub struct SessionCreation {
    branches: BranchAvailability,
    workspace: Arc<dyn Workspace>,
    multiplexer: Arc<dyn Multiplexer>,
    host: Arc<dyn HostProbe>,
    registry: Arc<dyn SessionRegistry>,
    clock: Arc<dyn Clock>,
    scratch: SessionScratch,
    hooks: Arc<AgentHooks>,
}

impl SessionCreation {
    pub fn new(ports: &ServicePorts, hooks: Arc<AgentHooks>) -> Self {
        Self {
            branches: BranchAvailability::new(ports),
            workspace: ports.repos().workspace(),
            multiplexer: ports.tmux().multiplexer(),
            host: ports.repos().host(),
            registry: ports.state().registry(),
            clock: ports.state().clock(),
            scratch: SessionScratch::new(ports),
            hooks,
        }
    }

    pub fn create(
        &self,
        request: &CreateRequest,
        reconciliation: &Reconciliation,
        config: &Config,
        config_path: &Path,
    ) -> Result<Session, ServiceError> {
        let plan = self.plan_creation(request, config, config_path)?;
        self.check_availability(&plan, reconciliation)?;
        let session_branches = RepoMerge::new(self.host.as_ref())
            .session_branches_in(plan.repo_path(), reconciliation.entries());
        let start = StartPointResolver::new(self.workspace.as_ref())
            .skipping(session_branches)
            .resolve(plan.repo_path(), plan.base_branch())?;
        let rollback =
            WorktreeRollback::new(self.workspace.as_ref(), self.host.as_ref(), &plan, &start);
        self.create_marked_worktree(&plan, &start, &rollback)?;
        let socket = self.start_tmux_session(&plan, &start, &rollback)?;
        let session = plan.session(&start, &socket, self.clock.now_epoch_seconds());
        let mut entries = reconciliation.entries().to_vec();
        entries.push(session.clone());
        self.registry.save(&entries).map_err(|error| {
            ServiceError::from(error)
                .adding_context("the session runs, and the next observe adopts it from tmux")
        })?;
        self.hooks.after_start(&session, plan.agent());
        Ok(session)
    }

    fn plan_creation(
        &self,
        request: &CreateRequest,
        config: &Config,
        config_path: &Path,
    ) -> Result<CreationPlan, ServiceError> {
        let checks = UseChecks::new(self.host.as_ref(), config, config_path);
        let agent = checks.agent_for_creation(request)?;
        let root = self
            .host
            .canonicalize_lenient(config.naming().worktree_root())?;
        Ok(CreationPlan::new(request, agent, config)?.rooted_at(&root))
    }

    fn check_availability(
        &self,
        plan: &CreationPlan,
        reconciliation: &Reconciliation,
    ) -> Result<(), ServiceError> {
        let taken_by_entry = reconciliation
            .entries()
            .iter()
            .any(|entry| entry.name() == plan.name());
        if taken_by_entry || reconciliation.is_live_tmux_name(&plan.tmux_name()) {
            return Err(ServiceError::Conflict(format!(
                "a session named {} already exists",
                plan.name()
            )));
        }
        self.check_worktree_available(plan, reconciliation)?;
        self.branches.refuse_taken(plan.repo_path(), &plan.branch())
    }

    fn check_worktree_available(
        &self,
        plan: &CreationPlan,
        reconciliation: &Reconciliation,
    ) -> Result<(), ServiceError> {
        let claimed_by_entry = reconciliation
            .entries()
            .iter()
            .any(|entry| entry.worktree_path() == plan.worktree());
        let canonical = self.host.canonicalize_lenient(&plan.worktree())?;
        let listed_by_git = self
            .workspace
            .list_worktrees(plan.repo_path())?
            .iter()
            .any(|entry| entry.path() == canonical);
        if claimed_by_entry || listed_by_git || self.host.path_exists(&plan.worktree()) {
            return Err(ServiceError::Conflict(format!(
                "worktree {} already exists",
                plan.worktree().display()
            )));
        }
        Ok(())
    }

    fn create_marked_worktree(
        &self,
        plan: &CreationPlan,
        start: &StartPoint,
        rollback: &WorktreeRollback,
    ) -> Result<(), ServiceError> {
        let branch = plan.branch();
        self.workspace
            .create_worktree(plan.repo_path(), &plan.worktree(), &branch, start)
            .and_then(|()| {
                self.workspace
                    .mark_branch_managed(plan.repo_path(), &branch)
            })
            .map_err(|error| rollback.run(error.into()))
    }

    fn start_tmux_session(
        &self,
        plan: &CreationPlan,
        start: &StartPoint,
        rollback: &WorktreeRollback,
    ) -> Result<PathBuf, ServiceError> {
        let launch = self.scratch.prepare(plan.launch());
        let spec = plan.spec(start, launch);
        self.multiplexer.create_session(&spec).map_err(|error| {
            if error.is_timeout() {
                self.after_tmux_timeout(plan, rollback, error.into())
            } else {
                self.roll_back_launch(plan, rollback, error.into())
            }
        })
    }

    fn after_tmux_timeout(
        &self,
        plan: &CreationPlan,
        rollback: &WorktreeRollback,
        error: ServiceError,
    ) -> ServiceError {
        match self.multiplexer.kill_session(&plan.tmux_name()) {
            Ok(()) => self.roll_back_launch(plan, rollback, error),
            Err(_) => error.adding_context(&format!(
                "kept the worktree {} and the branch {}, run ringleader doctor",
                plan.worktree().display(),
                plan.branch()
            )),
        }
    }

    fn roll_back_launch(
        &self,
        plan: &CreationPlan,
        rollback: &WorktreeRollback,
        error: ServiceError,
    ) -> ServiceError {
        self.scratch.discard(&plan.worktree());
        rollback.run(error)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::application::ports::{MultiplexerError, WorkspaceError, WorktreeEntry};
    use crate::domain::{Agent, AgentMode, AgentPane, BaseBranch, Branch, Repo, RepoName};
    use crate::test_support::{
        FakeHostProbe, FakeMultiplexer, FakeScratchSpace, FakeWorkspace, ServiceFixture,
    };

    struct CreateFixture;

    impl CreateFixture {
        const DEVELOP_TIP: &'static str = "5555555555555555555555555555555555555555";
        const RELEASE_TIP: &'static str = "6666666666666666666666666666666666666666";

        fn request(name: &str) -> CreateRequest {
            CreateRequest::new(name, ServiceFixture::repo())
        }

        fn repo_on_develop() -> Repo {
            Repo::explicit(
                RepoName::parse("example-project").unwrap(),
                Path::new(ServiceFixture::REPO),
                BaseBranch::new("develop"),
            )
        }

        fn with_local_tips(tips: &[(&str, &str)]) -> ServiceFixture {
            let local_tips: Vec<(Branch, &str)> = tips
                .iter()
                .map(|(name, tip)| (Branch::Local(name.to_string()), *tip))
                .collect();
            let workspace = FakeWorkspace::default()
                .with_branch_tips(Path::new(ServiceFixture::REPO), &local_tips);
            ServiceFixture::new().with_workspace(workspace)
        }

        fn branch(name: &str) -> String {
            ServiceFixture::session(name).branch().to_string()
        }

        fn has_marker(fixture: &ServiceFixture, name: &str) -> bool {
            fixture
                .workspace()
                .has_marker(Path::new(ServiceFixture::REPO), &Self::branch(name))
        }

        fn git_failure() -> WorkspaceError {
            WorkspaceError::Failed {
                subcommand: "worktree".to_string(),
                status: "exit status: 128".to_string(),
                stderr: "fatal: hook failed".to_string(),
            }
        }

        fn tmux_failure() -> MultiplexerError {
            MultiplexerError::Failed {
                subcommand: "new-session".to_string(),
                detail: "duplicate session".to_string(),
            }
        }

        fn tmux_timeout() -> MultiplexerError {
            MultiplexerError::TimedOut {
                subcommand: "new-session".to_string(),
                seconds: 10,
            }
        }

        fn create(
            fixture: &ServiceFixture,
            request: CreateRequest,
        ) -> Result<Session, ServiceError> {
            fixture.service().create_session(&request)
        }

        fn assert_nothing_touched(fixture: &ServiceFixture) {
            assert!(fixture.workspace().state_changing_calls().is_empty());
            assert!(fixture.multiplexer().state_changing_calls().is_empty());
        }
    }

    #[test]
    fn create_records_the_session_with_the_socket_tmux_reports() {
        let fixture = ServiceFixture::new();

        let session = CreateFixture::create(&fixture, CreateFixture::request("Fix Auth")).unwrap();

        assert_eq!(session.name().as_str(), "fix-auth");
        assert_eq!(session.socket_path(), Path::new(FakeMultiplexer::SOCKET));
        assert_eq!(fixture.registry().sessions(), vec![session]);
    }

    #[test]
    fn create_builds_the_worktree_from_the_resolved_start_point() {
        let fixture = ServiceFixture::new();

        CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap();

        assert_eq!(
            fixture.workspace().state_changing_calls(),
            [
                format!(
                    "create_worktree {} fix-auth",
                    ServiceFixture::worktree_path("fix-auth").display()
                ),
                "mark_branch_managed fix-auth".to_string()
            ]
        );
    }

    #[test]
    fn a_configured_base_branch_tip_becomes_the_recorded_start() {
        let fixture = CreateFixture::with_local_tips(&[("develop", CreateFixture::DEVELOP_TIP)]);
        let request = CreateFixture::request("fix-auth")
            .with_base_branch(BaseBranch::new("develop").unwrap());

        let session = CreateFixture::create(&fixture, request).unwrap();

        assert_eq!(session.base_commit(), CreateFixture::DEVELOP_TIP);
    }

    #[test]
    fn the_repo_base_branch_is_started_from_without_a_requested_one() {
        let fixture = CreateFixture::with_local_tips(&[
            ("main", FakeWorkspace::DEFAULT_START),
            ("develop", CreateFixture::DEVELOP_TIP),
        ]);
        let request = CreateRequest::new("fix-auth", CreateFixture::repo_on_develop());

        let session = CreateFixture::create(&fixture, request).unwrap();

        assert_eq!(session.base_commit(), CreateFixture::DEVELOP_TIP);
    }

    #[test]
    fn a_requested_base_branch_is_started_from_over_the_repo_one() {
        let fixture = CreateFixture::with_local_tips(&[
            ("develop", CreateFixture::DEVELOP_TIP),
            ("release", CreateFixture::RELEASE_TIP),
        ]);
        let request = CreateRequest::new("fix-auth", CreateFixture::repo_on_develop())
            .with_base_branch(BaseBranch::new("release").unwrap());

        let session = CreateFixture::create(&fixture, request).unwrap();

        assert_eq!(session.base_commit(), CreateFixture::RELEASE_TIP);
    }

    #[test]
    fn create_records_a_requested_base_branch_on_the_session() {
        let develop = BaseBranch::new("develop").unwrap();
        let fixture =
            ServiceFixture::new().with_workspace(FakeWorkspace::default().with_branch_tip(
                Path::new(ServiceFixture::REPO),
                Branch::Local("develop".to_string()),
                FakeWorkspace::DEFAULT_START,
            ));
        let request = CreateFixture::request("fix-auth").with_base_branch(develop.clone());

        let session = CreateFixture::create(&fixture, request).unwrap();

        assert_eq!(session.base_branch(), Some(&develop));
    }

    #[test]
    fn create_leaves_a_configured_base_branch_off_the_session() {
        let fixture =
            ServiceFixture::new().with_workspace(FakeWorkspace::default().with_branch_tip(
                Path::new(ServiceFixture::REPO),
                Branch::Local("develop".to_string()),
                FakeWorkspace::DEFAULT_START,
            ));
        let request = CreateRequest::new("fix-auth", CreateFixture::repo_on_develop());

        let session = CreateFixture::create(&fixture, request).unwrap();

        assert_eq!(session.base_branch(), None);
    }

    #[test]
    fn a_missing_configured_base_branch_is_refused_before_anything_is_touched() {
        let fixture = ServiceFixture::new();
        let request = CreateFixture::request("fix-auth")
            .with_base_branch(BaseBranch::new("develop").unwrap());

        let error = CreateFixture::create(&fixture, request).unwrap_err();

        assert!(matches!(error, ServiceError::Invalid(_)));
        CreateFixture::assert_nothing_touched(&fixture);
    }

    #[test]
    fn a_new_session_never_starts_from_another_sessions_branch() {
        let master_tip = "7777777777777777777777777777777777777777";
        let repo = Path::new(ServiceFixture::REPO);
        let workspace = FakeWorkspace::default()
            .with_branch_tip(
                repo,
                Branch::Local("main".to_string()),
                FakeWorkspace::DEFAULT_START,
            )
            .with_branch_tip(repo, Branch::Local("master".to_string()), master_tip);
        let fixture = ServiceFixture::new()
            .with_sessions(&[ServiceFixture::session("main")])
            .with_workspace(workspace);

        let session = CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap();

        assert_eq!(session.base_commit(), master_tip);
    }

    #[test]
    fn create_marks_the_branch_it_creates() {
        let fixture = ServiceFixture::new();

        CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap();

        assert!(CreateFixture::has_marker(&fixture, "fix-auth"));
    }

    #[test]
    fn a_failed_marking_removes_the_worktree_and_branch() {
        let fixture = ServiceFixture::new().with_workspace(
            FakeWorkspace::default().failing("mark_branch_managed", CreateFixture::git_failure()),
        );

        CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap_err();

        let repo = Path::new(ServiceFixture::REPO);
        let worktree = ServiceFixture::worktree_path("fix-auth");
        let branch = CreateFixture::branch("fix-auth");
        assert!(!fixture.workspace().has_branch(repo, &branch));
        assert!(!fixture.workspace().lists_worktree(repo, &worktree));
        assert!(fixture.multiplexer().state_changing_calls().is_empty());
    }

    #[test]
    fn a_failed_orphan_creation_leaves_no_marker() {
        let fixture = ServiceFixture::new()
            .with_workspace(FakeWorkspace::default().starting_orphan())
            .with_multiplexer(
                FakeMultiplexer::new().failing("create_session", CreateFixture::tmux_failure()),
            );

        CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap_err();

        assert!(!CreateFixture::has_marker(&fixture, "fix-auth"));
    }

    #[test]
    fn create_uses_the_first_agent_by_default() {
        let config = ServiceFixture::config().with_agents(vec![
            ServiceFixture::agent(),
            Agent::new("opencode", "opencode"),
        ]);
        let fixture = ServiceFixture::new().with_config(config);

        let session = CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap();

        assert_eq!(session.agent(), "claude");
    }

    #[test]
    fn create_launches_the_agent_with_its_expanded_prompt() {
        let fixture = ServiceFixture::new();

        CreateFixture::create(
            &fixture,
            CreateFixture::request("fix-auth").with_prompt("fix the tests; then stop"),
        )
        .unwrap();

        let launch = fixture.multiplexer().launches().pop().unwrap();
        assert_eq!(launch.argv(), ["claude", "fix the tests; then stop"]);
    }

    #[test]
    fn create_in_plan_mode_launches_the_agent_with_its_plan_args() {
        let fixture = ServiceFixture::new();

        CreateFixture::create(
            &fixture,
            CreateFixture::request("fix-auth")
                .with_mode(AgentMode::Plan)
                .with_prompt("fix the tests"),
        )
        .unwrap();

        let launch = fixture.multiplexer().launches().pop().unwrap();
        assert_eq!(
            launch.argv(),
            ["claude", "--permission-mode", "plan", "fix the tests"]
        );
    }

    #[test]
    fn create_passes_the_session_name_to_the_agent() {
        let fixture = ServiceFixture::new().with_config(ServiceFixture::config_with_input());

        CreateFixture::create(&fixture, CreateFixture::request("Fix Auth")).unwrap();

        let launch = fixture.multiplexer().launches().pop().unwrap();
        assert_eq!(launch.argv(), ["claude", "--name", "fix-auth"]);
    }

    #[test]
    fn create_types_the_startup_input_into_the_new_session() {
        let fixture = ServiceFixture::new().with_config(ServiceFixture::config_with_input());

        CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap();

        assert!(
            fixture
                .multiplexer()
                .wait_for_typing("ringleader-fix-auth", Duration::from_secs(2))
        );
        assert_eq!(
            fixture
                .multiplexer()
                .typed_when_ready("ringleader-fix-auth"),
            ["/color"]
        );
    }

    #[test]
    fn create_returns_before_the_startup_input_is_typed() {
        let fixture = ServiceFixture::new()
            .with_config(ServiceFixture::config_with_input())
            .with_multiplexer(FakeMultiplexer::new().blocking_typing());
        let mut service = fixture.service();
        let started = Instant::now();

        service
            .create_session(&CreateFixture::request("fix-auth"))
            .unwrap();

        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(
            fixture
                .multiplexer()
                .typed_when_ready("ringleader-fix-auth")
                .is_empty()
        );
        fixture.multiplexer().release_typing();
        service.wait_startup_input(|| false);
        assert_eq!(
            fixture
                .multiplexer()
                .typed_when_ready("ringleader-fix-auth"),
            ["/color"]
        );
    }

    #[test]
    fn create_types_nothing_for_an_agent_without_startup_input() {
        let fixture = ServiceFixture::new();

        CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap();

        assert!(
            fixture
                .multiplexer()
                .calls()
                .iter()
                .all(|call| !call.starts_with("type_when_ready"))
        );
    }

    #[test]
    fn an_agent_that_never_reaches_its_prompt_still_gets_its_session_recorded() {
        let fixture = ServiceFixture::new()
            .with_config(ServiceFixture::config_with_input())
            .with_multiplexer(FakeMultiplexer::new().failing(
                "type_when_ready",
                MultiplexerError::AgentNotReady {
                    tmux_name: "ringleader-fix-auth".to_string(),
                },
            ));

        let session = CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap();

        assert_eq!(fixture.registry().sessions(), vec![session]);
    }

    #[test]
    fn create_launches_the_agent_with_its_set_env() {
        let assignments = [(
            "EXAMPLE_ACCOUNT_DIR".to_string(),
            "/accounts/example".to_string(),
        )];
        let agent = ServiceFixture::agent().with_env_assignments(&assignments);
        let fixture =
            ServiceFixture::new().with_config(ServiceFixture::config().with_agents(vec![agent]));

        CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap();

        let launch = fixture.multiplexer().launches().pop().unwrap();
        assert!(launch.env_assignments().starts_with(&assignments));
    }

    #[test]
    fn create_points_the_agent_tmpdir_at_the_session_scratch_directory() {
        let fixture = ServiceFixture::new();

        CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap();

        let launch = fixture.multiplexer().launches().pop().unwrap();
        assert_eq!(
            launch.env_assignments(),
            [fixture.tmpdir_assignment("fix-auth")]
        );
    }

    #[test]
    fn a_scratch_directory_that_cannot_be_made_still_creates_the_session() {
        let fixture =
            ServiceFixture::new().with_scratch_space(FakeScratchSpace::default().failing("create"));

        let session = CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap();

        assert_eq!(fixture.registry().sessions(), vec![session]);
        let launch = fixture.multiplexer().launches().pop().unwrap();
        assert!(launch.env_assignments().is_empty());
    }

    #[test]
    fn the_session_spec_carries_inherited_names_and_never_their_values() {
        let fixture = ServiceFixture::new();

        CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap();

        let spec = fixture.multiplexer().specs().pop().unwrap();
        assert_eq!(spec.launch().env_names(), ["ANTHROPIC_API_KEY"]);
        assert_eq!(spec.removed_env_names(), ["ANTHROPIC_API_KEY"]);
        let assigned = spec.launch().env_assignments();
        assert!(assigned.iter().all(|(name, _)| name != "ANTHROPIC_API_KEY"));
    }

    #[test]
    fn the_session_spec_carries_the_configured_key_bindings() {
        let fixture = ServiceFixture::new().with_config(ServiceFixture::config_with_alt_n());

        CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap();

        let spec = fixture.multiplexer().specs().pop().unwrap();
        assert_eq!(spec.key_bindings(), &ServiceFixture::alt_n_bindings());
    }

    #[test]
    fn create_never_stores_the_prompt() {
        let fixture = ServiceFixture::new();

        let session = CreateFixture::create(
            &fixture,
            CreateFixture::request("fix-auth").with_prompt("secret plan"),
        )
        .unwrap();

        assert!(!format!("{session:?}").contains("secret plan"));
    }

    #[test]
    fn create_refuses_a_slug_a_registry_entry_uses() {
        let existing = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new().with_sessions(&[existing]);

        let error =
            CreateFixture::create(&fixture, CreateFixture::request("FIX_AUTH")).unwrap_err();

        assert!(matches!(error, ServiceError::Conflict(_)));
        CreateFixture::assert_nothing_touched(&fixture);
    }

    #[test]
    fn create_refuses_a_slug_a_live_tmux_session_uses() {
        let live = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new().with_multiplexer(
            FakeMultiplexer::new().with_live(ServiceFixture::observed(&live, AgentPane::live(0))),
        );

        let error =
            CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap_err();

        assert!(matches!(error, ServiceError::Conflict(_)));
        CreateFixture::assert_nothing_touched(&fixture);
    }

    #[test]
    fn create_refuses_an_existing_worktree_directory() {
        let fixture = ServiceFixture::new().with_host(
            ServiceFixture::host_for(&[])
                .with_directory(&ServiceFixture::worktree_path("fix-auth")),
        );

        let error =
            CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap_err();

        assert!(error.to_string().contains("worktree"));
        CreateFixture::assert_nothing_touched(&fixture);
    }

    #[test]
    fn create_refuses_a_worktree_path_git_still_lists() {
        let repo = Path::new(ServiceFixture::REPO);
        let fixture = ServiceFixture::new().with_workspace(FakeWorkspace::default().with_worktree(
            repo,
            WorktreeEntry::new(&ServiceFixture::worktree_path("fix-auth")),
        ));

        let error =
            CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap_err();

        assert!(matches!(error, ServiceError::Conflict(_)));
        CreateFixture::assert_nothing_touched(&fixture);
    }

    #[test]
    fn create_refuses_an_existing_branch() {
        let fixture = ServiceFixture::new().with_workspace(FakeWorkspace::default().with_branch(
            Path::new(ServiceFixture::REPO),
            "fix-auth",
            ServiceFixture::TIP,
        ));

        let error =
            CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap_err();

        assert!(error.to_string().contains("branch fix-auth already exists"));
        CreateFixture::assert_nothing_touched(&fixture);
    }

    #[test]
    fn create_refuses_a_name_that_holds_other_branches() {
        let fixture = ServiceFixture::new().with_workspace(FakeWorkspace::default().with_branch(
            Path::new(ServiceFixture::REPO),
            "fix-auth/draft",
            ServiceFixture::TIP,
        ));

        let error =
            CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap_err();

        assert!(matches!(error, ServiceError::Conflict(_)));
        assert!(
            error
                .to_string()
                .contains("branch fix-auth clashes with existing branch fix-auth/draft")
        );
        CreateFixture::assert_nothing_touched(&fixture);
    }

    #[test]
    fn a_failed_worktree_creation_removes_what_git_left() {
        let fixture = ServiceFixture::new().with_workspace(
            FakeWorkspace::default()
                .failing("create_worktree", CreateFixture::git_failure())
                .leaving_worktree_after_failed_create(),
        );

        CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap_err();

        let repo = Path::new(ServiceFixture::REPO);
        assert!(!fixture.workspace().has_branch(repo, "fix-auth"));
        assert!(
            !fixture
                .workspace()
                .lists_worktree(repo, &ServiceFixture::worktree_path("fix-auth"))
        );
    }

    #[test]
    fn a_failed_creation_that_left_a_locked_worktree_unlocks_it_before_removal() {
        let fixture = ServiceFixture::new().with_workspace(
            FakeWorkspace::default()
                .failing("create_worktree", CreateFixture::git_failure())
                .leaving_locked_worktree_after_failed_create(),
        );

        CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap_err();

        let calls = fixture.workspace().state_changing_calls();
        let operations: Vec<&str> = calls
            .iter()
            .map(|call| call.split(' ').next().unwrap())
            .collect();
        assert_eq!(
            operations,
            [
                "create_worktree",
                "unlock_worktree",
                "remove_worktree",
                "delete_branch"
            ]
        );
    }

    #[test]
    fn a_failed_tmux_creation_removes_the_worktree_branch_and_scratch_directory() {
        let fixture = ServiceFixture::new().with_multiplexer(
            FakeMultiplexer::new().failing("create_session", CreateFixture::tmux_failure()),
        );

        let error =
            CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap_err();

        assert!(error.to_string().contains("duplicate session"));
        let repo = Path::new(ServiceFixture::REPO);
        assert!(!fixture.workspace().has_branch(repo, "fix-auth"));
        assert!(fixture.registry().sessions().is_empty());
        assert_eq!(
            fixture.scratch_space().removed(),
            [ServiceFixture::worktree_path("fix-auth")]
        );
    }

    #[test]
    fn a_timed_out_tmux_creation_whose_kill_completes_removes_the_worktree_branch_and_scratch() {
        let fixture = ServiceFixture::new().with_multiplexer(
            FakeMultiplexer::new()
                .failing("create_session", CreateFixture::tmux_timeout())
                .creating_despite_failure(),
        );

        CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap_err();

        let repo = Path::new(ServiceFixture::REPO);
        assert!(fixture.multiplexer().live_names().is_empty());
        assert!(!fixture.workspace().has_branch(repo, "fix-auth"));
        assert_eq!(
            fixture.scratch_space().removed(),
            [ServiceFixture::worktree_path("fix-auth")]
        );
    }

    #[test]
    fn a_timed_out_tmux_creation_whose_kill_also_times_out_keeps_the_worktree_branch_and_scratch() {
        let fixture = ServiceFixture::new().with_multiplexer(
            FakeMultiplexer::new()
                .failing("create_session", CreateFixture::tmux_timeout())
                .failing("kill_session", CreateFixture::tmux_timeout()),
        );

        let error =
            CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap_err();

        let repo = Path::new(ServiceFixture::REPO);
        assert!(fixture.workspace().has_branch(repo, "fix-auth"));
        assert!(error.to_string().contains("doctor"));
        assert!(fixture.scratch_space().removed().is_empty());
    }

    #[test]
    fn create_without_agents_refuses_naming_the_config_path() {
        let fixture =
            ServiceFixture::new().with_config(ServiceFixture::config().with_agents(vec![]));

        let error =
            CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap_err();

        assert_eq!(
            error.to_string(),
            "no agents configured in /config/ringleader/config.toml"
        );
        CreateFixture::assert_nothing_touched(&fixture);
    }

    #[test]
    fn create_refuses_an_agent_command_that_does_not_resolve() {
        let fixture = ServiceFixture::new().with_host(
            FakeHostProbe::default()
                .with_directory(Path::new(ServiceFixture::REPO))
                .with_directory(Path::new(ServiceFixture::DATA_DIR)),
        );

        let error =
            CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap_err();

        assert!(matches!(error, ServiceError::Invalid(_)));
        CreateFixture::assert_nothing_touched(&fixture);
    }

    #[test]
    fn an_unresolved_command_with_flags_says_to_move_them_into_args() {
        let agent = Agent::new("claude", "claude --model opus");
        let fixture =
            ServiceFixture::new().with_config(ServiceFixture::config().with_agents(vec![agent]));

        let error =
            CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap_err();

        assert!(error.to_string().contains("move the flags into args"));
    }

    #[test]
    fn create_refuses_an_unwritable_worktree_root() {
        let fixture = ServiceFixture::new().with_host(
            ServiceFixture::host_for(&[]).with_unwritable(Path::new(ServiceFixture::DATA_DIR)),
        );

        let error =
            CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap_err();

        assert!(error.to_string().contains("is not writable"));
        CreateFixture::assert_nothing_touched(&fixture);
    }

    #[test]
    fn create_refuses_a_prompt_for_an_agent_without_a_placeholder() {
        let agent = Agent::new("claude", "claude");
        let fixture =
            ServiceFixture::new().with_config(ServiceFixture::config().with_agents(vec![agent]));

        let error = CreateFixture::create(
            &fixture,
            CreateFixture::request("fix-auth").with_prompt("go"),
        )
        .unwrap_err();

        assert!(matches!(error, ServiceError::Invalid(_)));
        CreateFixture::assert_nothing_touched(&fixture);
    }

    #[test]
    fn create_refuses_plan_mode_for_an_agent_without_plan_args() {
        let agent = Agent::new("claude", "claude");
        let fixture =
            ServiceFixture::new().with_config(ServiceFixture::config().with_agents(vec![agent]));

        let error = CreateFixture::create(
            &fixture,
            CreateFixture::request("fix-auth").with_mode(AgentMode::Plan),
        )
        .unwrap_err();

        assert!(error.to_string().contains("cannot start in plan mode"));
        CreateFixture::assert_nothing_touched(&fixture);
    }
}
