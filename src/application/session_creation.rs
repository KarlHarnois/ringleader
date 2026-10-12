use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::agent_hooks::AgentHooks;
use super::creation_plan::{CreateRequest, CreationPlan};
use super::ports::{Clock, HostProbe, Multiplexer, SessionRegistry, StartPoint, Workspace};
use super::reconciliation::Reconciliation;
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use super::use_checks::UseChecks;
use super::worktree_rollback::WorktreeRollback;
use crate::domain::{Config, Session};

pub struct SessionCreation {
    workspace: Arc<dyn Workspace>,
    multiplexer: Arc<dyn Multiplexer>,
    host: Arc<dyn HostProbe>,
    registry: Arc<dyn SessionRegistry>,
    clock: Arc<dyn Clock>,
    hooks: AgentHooks,
}

impl SessionCreation {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            workspace: ports.repos().workspace(),
            multiplexer: ports.tmux().multiplexer(),
            host: ports.repos().host(),
            registry: ports.state().registry(),
            clock: ports.state().clock(),
            hooks: AgentHooks::new(ports),
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
        let start = self
            .workspace
            .resolve_start_point(plan.repo_path(), plan.base_branch())?;
        let rollback =
            WorktreeRollback::new(self.workspace.as_ref(), self.host.as_ref(), &plan, &start);
        self.workspace
            .create_worktree(plan.repo_path(), &plan.worktree(), &plan.branch(), &start)
            .map_err(|error| rollback.run(error.into()))?;
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
        let agent = checks.agent_for_creation(request.agent(), request.prompt())?;
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
        if self
            .workspace
            .branch_exists(plan.repo_path(), &plan.branch())?
        {
            return Err(ServiceError::Conflict(format!(
                "branch {} already exists",
                plan.branch()
            )));
        }
        Ok(())
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

    fn start_tmux_session(
        &self,
        plan: &CreationPlan,
        start: &StartPoint,
        rollback: &WorktreeRollback,
    ) -> Result<PathBuf, ServiceError> {
        let spec = plan.spec(start);
        self.multiplexer.create_session(&spec).map_err(|error| {
            if error.is_timeout() {
                self.after_tmux_timeout(plan, rollback, error.into())
            } else {
                rollback.run(error.into())
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
            Ok(()) => rollback.run(error),
            Err(_) => error.adding_context(&format!(
                "kept the worktree {} and the branch {}, run ringleader doctor",
                plan.worktree().display(),
                plan.branch()
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::application::ports::{MultiplexerError, WorkspaceError, WorktreeEntry};
    use crate::domain::Agent;
    use crate::domain::AgentPane;
    use crate::test_support::{FakeHostProbe, FakeMultiplexer, FakeWorkspace, ServiceFixture};

    struct CreateFixture;

    impl CreateFixture {
        fn request(name: &str) -> CreateRequest {
            CreateRequest::new(name, ServiceFixture::repo())
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
            [format!(
                "create_worktree {} ringleader/fix-auth",
                ServiceFixture::worktree_path("fix-auth").display()
            )]
        );
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
        assert_eq!(launch.env_assignments(), assignments);
    }

    #[test]
    fn the_session_spec_carries_inherited_names_and_never_their_values() {
        let fixture = ServiceFixture::new();

        CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap();

        let spec = fixture.multiplexer().specs().pop().unwrap();
        assert_eq!(spec.launch().env_names(), ["ANTHROPIC_API_KEY"]);
        assert_eq!(spec.removed_env_names(), ["ANTHROPIC_API_KEY"]);
        assert!(spec.launch().env_assignments().is_empty());
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

        assert_eq!(error.exit_code(), 3);
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

        assert_eq!(error.exit_code(), 3);
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

        assert_eq!(error.exit_code(), 3);
        CreateFixture::assert_nothing_touched(&fixture);
    }

    #[test]
    fn create_refuses_an_existing_branch() {
        let fixture = ServiceFixture::new().with_workspace(FakeWorkspace::default().with_branch(
            Path::new(ServiceFixture::REPO),
            "ringleader/fix-auth",
            ServiceFixture::TIP,
        ));

        let error =
            CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("branch ringleader/fix-auth already exists")
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
        assert!(!fixture.workspace().has_branch(repo, "ringleader/fix-auth"));
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
    fn a_failed_tmux_creation_removes_the_worktree_and_branch() {
        let fixture = ServiceFixture::new().with_multiplexer(
            FakeMultiplexer::new().failing("create_session", CreateFixture::tmux_failure()),
        );

        let error =
            CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap_err();

        assert!(error.to_string().contains("duplicate session"));
        let repo = Path::new(ServiceFixture::REPO);
        assert!(!fixture.workspace().has_branch(repo, "ringleader/fix-auth"));
        assert!(fixture.registry().sessions().is_empty());
    }

    #[test]
    fn a_timed_out_tmux_creation_whose_kill_completes_removes_the_worktree_and_branch() {
        let fixture = ServiceFixture::new().with_multiplexer(
            FakeMultiplexer::new()
                .failing("create_session", CreateFixture::tmux_timeout())
                .creating_despite_failure(),
        );

        CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap_err();

        let repo = Path::new(ServiceFixture::REPO);
        assert!(fixture.multiplexer().live_names().is_empty());
        assert!(!fixture.workspace().has_branch(repo, "ringleader/fix-auth"));
    }

    #[test]
    fn a_timed_out_tmux_creation_whose_kill_also_times_out_keeps_the_worktree_and_branch() {
        let fixture = ServiceFixture::new().with_multiplexer(
            FakeMultiplexer::new()
                .failing("create_session", CreateFixture::tmux_timeout())
                .failing("kill_session", CreateFixture::tmux_timeout()),
        );

        let error =
            CreateFixture::create(&fixture, CreateFixture::request("fix-auth")).unwrap_err();

        let repo = Path::new(ServiceFixture::REPO);
        assert!(fixture.workspace().has_branch(repo, "ringleader/fix-auth"));
        assert!(error.to_string().contains("doctor"));
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

        assert_eq!(error.exit_code(), 1);
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

        assert_eq!(error.exit_code(), 1);
        CreateFixture::assert_nothing_touched(&fixture);
    }
}
