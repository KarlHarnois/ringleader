use std::collections::HashMap;

use super::delete_report::DeleteReport;
use super::list_report::ListReport;
use super::repo_argument::RepoArgument;
use super::{
    CliExit, CliSession, CommandStop, DeleteArgs, ListArgs, NewArgs, RenameArgs, RestartArgs,
    SendArgs, StopArgs,
};
use crate::application::{
    CreateRequest, DeleteConfirmation, DeleteImpact, DeleteOutcome, RestartReport, ServiceError,
    SessionService, StartOutcome, StopReport,
};
use crate::domain::{AgentMode, BaseBranch, Checkout, Repo, RepoGroup};

pub struct SessionCommands<'a> {
    service: &'a mut SessionService,
    session: &'a mut CliSession,
}

impl<'a> SessionCommands<'a> {
    const NOTHING_TO_RESTART: &'static str = "no exited or stopped session to restart";
    const NOTHING_TO_STOP: &'static str = "no waiting or exited session to stop";

    pub fn new(service: &'a mut SessionService, session: &'a mut CliSession) -> Self {
        Self { service, session }
    }

    pub fn list(&mut self, args: &ListArgs) -> Result<u8, CommandStop> {
        let rows = self.service.observe()?;
        let names: Vec<String> = rows
            .iter()
            .filter(|row| row.session().is_some())
            .map(|row| row.name().to_string())
            .collect();
        let diffs: HashMap<String, _> = self
            .service
            .diff_stats(&names)
            .into_iter()
            .filter_map(|(name, diff)| Some((name, diff.ok()?)))
            .collect();
        let report = ListReport::new(&rows, &diffs);
        if args.json {
            self.session.output().line(&report.json());
        } else {
            for line in report.lines() {
                self.session.output().line(&line);
            }
        }
        Ok(0)
    }

    pub fn create(&mut self, args: &NewArgs) -> Result<u8, CommandStop> {
        self.service.observe_and_record()?;
        let repo = RepoArgument::new(&args.repo).resolve(self.service, self.session)?;
        let request = Self::create_request(args, repo);
        self.session.stop_if_signaled()?;
        let outcome = self.service.create_session(&request)?;
        let created = outcome.session();
        self.session.output().line(&format!(
            "created {} in {} on branch {}",
            created.name(),
            created.worktree_path().display(),
            created.branch()
        ));
        self.note_if_present(outcome.warning());
        Ok(0)
    }

    fn create_request(args: &NewArgs, repo: Repo) -> CreateRequest {
        let mut request = CreateRequest::new(&args.name, repo)
            .with_checkout(Self::checkout(args))
            .with_mode(Self::mode(args))
            .with_prompt(args.prompt.as_deref().unwrap_or(""));
        if let Some(agent) = &args.agent {
            request = request.with_agent(agent);
        }
        if let Some(base) = args.base_branch.as_deref().and_then(BaseBranch::new) {
            request = request.with_base_branch(base);
        }
        request
    }

    fn mode(args: &NewArgs) -> AgentMode {
        if args.plan {
            AgentMode::Plan
        } else {
            AgentMode::Build
        }
    }

    fn checkout(args: &NewArgs) -> Checkout {
        if args.main_checkout {
            Checkout::Main
        } else {
            Checkout::NewWorktree
        }
    }

    pub fn delete(&mut self, args: &DeleteArgs) -> Result<u8, CommandStop> {
        self.service.observe_and_record()?;
        let confirmation = Self::delete_confirmation(args);
        self.session.stop_if_signaled()?;
        let confirmations: Vec<(String, DeleteConfirmation)> = args
            .names
            .iter()
            .map(|name| (name.clone(), confirmation.clone()))
            .collect();
        let session = &*self.session;
        let report = self
            .service
            .delete_sessions(&confirmations, || session.is_signaled());
        for (name, outcome) in report.outcomes() {
            self.report_delete(name, outcome);
        }
        Ok(Self::failure_code(report.failures()))
    }

    fn report_delete(&mut self, name: &str, outcome: Result<&DeleteOutcome, &ServiceError>) {
        let report = DeleteReport::new(name);
        match outcome {
            Ok(outcome) => self.report_deleted(&report, outcome),
            Err(ServiceError::GuardsChanged { impact, .. }) => self.report_refusal(&report, impact),
            Err(error) => self.report_failure(name, error),
        }
    }

    fn delete_confirmation(args: &DeleteArgs) -> DeleteConfirmation {
        let confirmation = match args.force {
            true => DeleteConfirmation::force(),
            false => DeleteConfirmation::confirmed(DeleteImpact::default()),
        };
        match args.forget {
            true => confirmation.forgetting(),
            false => confirmation,
        }
    }

    fn report_deleted(&mut self, report: &DeleteReport, outcome: &DeleteOutcome) {
        for warning in report.warnings(outcome) {
            self.session.output().note(&warning);
        }
        self.session.output().line(&report.success(outcome));
    }

    fn report_refusal(&mut self, report: &DeleteReport, impact: &DeleteImpact) {
        for line in report.refusal(impact) {
            self.session.output().note(&line);
        }
    }

    pub fn attach(&mut self, name: &str) -> Result<u8, CommandStop> {
        self.service.attach(name)?;
        if let Some(requested) = self.take_requested_deletion() {
            self.session
                .output()
                .line(&format!("run ringleader delete {requested}"));
        }
        Ok(0)
    }

    fn take_requested_deletion(&mut self) -> Option<String> {
        let requested = self.service.requested_deletion();
        if let Err(error) = self.service.clear_return_request() {
            log::warn!("could not clear the return request: {error}");
        }
        requested
            .inspect_err(|error| log::warn!("could not read the return request: {error}"))
            .ok()
            .flatten()
    }

    pub fn send(&mut self, args: &SendArgs) -> Result<u8, CommandStop> {
        self.service.send_prompt(&args.name, &args.prompt)?;
        self.session
            .output()
            .line(&format!("sent the prompt to {}", args.name));
        Ok(0)
    }

    pub fn rename(&mut self, args: &RenameArgs) -> Result<u8, CommandStop> {
        self.service.observe_and_record()?;
        self.session.stop_if_signaled()?;
        let renamed = self.service.rename_session(&args.old, &args.new)?;
        self.session
            .output()
            .line(&format!("renamed {} to {}", args.old, renamed.name()));
        Ok(0)
    }

    pub fn restart_named_or_all(&mut self, args: &RestartArgs) -> Result<u8, CommandStop> {
        match args.all {
            true => self.restart_all(),
            false => self.restart(&args.names),
        }
    }

    pub fn restart(&mut self, names: &[String]) -> Result<u8, CommandStop> {
        self.service.observe_and_record()?;
        self.session.stop_if_signaled()?;
        let session = &*self.session;
        let report = self
            .service
            .restart_sessions(names, || session.is_signaled());
        self.report_restarts(&report);
        Ok(Self::failure_code(report.failures()))
    }

    pub fn stop_named_or_repo(&mut self, args: &StopArgs) -> Result<u8, CommandStop> {
        match &args.repo {
            Some(repo) => self.stop_repo(repo),
            None => self.stop(&args.names),
        }
    }

    pub fn stop(&mut self, names: &[String]) -> Result<u8, CommandStop> {
        self.service.observe_and_record()?;
        self.session.stop_if_signaled()?;
        let session = &*self.session;
        let report = self.service.stop_sessions(names, || session.is_signaled());
        self.report_stopped(report.outcomes());
        Ok(Self::failure_code(report.failures()))
    }

    pub fn stop_repo(&mut self, repo: &str) -> Result<u8, CommandStop> {
        self.service.observe_and_record()?;
        let repo = RepoArgument::new(repo).resolve(self.service, self.session)?;
        self.session.stop_if_signaled()?;
        let session = &*self.session;
        let report = self
            .service
            .stop_repo(&RepoGroup::of_repo(&repo), || session.is_signaled())?;
        self.report_stops(&report);
        Ok(Self::failure_code(report.failures()))
    }

    fn report_stops(&mut self, report: &StopReport) {
        for (name, skip) in report.skips() {
            self.session
                .output()
                .note(&format!("skipped {name}, {}", skip.label()));
        }
        if report.outcomes().next().is_none() {
            self.session.output().note(Self::NOTHING_TO_STOP);
        }
        self.report_stopped(report.outcomes());
    }

    fn report_stopped<'r>(
        &mut self,
        outcomes: impl Iterator<Item = (&'r str, Result<&'r (), &'r ServiceError>)>,
    ) {
        for (name, stop) in outcomes {
            match stop {
                Ok(()) => self.session.output().line(&format!("stopped {name}")),
                Err(error) => self.report_failure(name, error),
            }
        }
    }

    pub fn restart_all(&mut self) -> Result<u8, CommandStop> {
        self.service.observe_and_record()?;
        self.session.stop_if_signaled()?;
        let session = &*self.session;
        let report = self.service.restart_all(|| session.is_signaled())?;
        self.report_restarts(&report);
        Ok(Self::failure_code(report.failures()))
    }

    fn report_restarts(&mut self, report: &RestartReport) {
        if report.is_empty() {
            self.session.output().note(Self::NOTHING_TO_RESTART);
        }
        for (name, restart) in report.outcomes() {
            match restart {
                Ok(outcome) => self.report_restart(name, outcome),
                Err(error) => self.report_failure(name, error),
            }
        }
    }

    fn report_restart(&mut self, name: &str, outcome: &StartOutcome) {
        self.note_if_present(outcome.recovery());
        self.note_if_present(outcome.warning());
        self.session.output().line(&format!("restarted {name}"));
    }

    fn report_failure(&mut self, name: &str, error: &ServiceError) {
        self.session.output().error(&format!("{name}: {error}"));
    }

    fn failure_code<'e>(mut failures: impl Iterator<Item = (&'e str, &'e ServiceError)>) -> u8 {
        failures
            .next()
            .map_or(0, |(_, error)| CliExit::error_code(error))
    }

    fn note_if_present(&mut self, note: Option<&str>) {
        if let Some(note) = note {
            self.session.output().note(note);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use signal_hook::consts::{SIGHUP, SIGINT};

    use crate::application::ports::{
        AttachTarget, ConfigError, DirtyState, MultiplexerError, RegistryError, ReturnRequest,
    };
    use crate::cli::CliExit;
    use crate::domain::{Repo, RepoName, Session};
    use crate::test_support::{
        CapturedWriter, CliHarness, FakeMultiplexer, FakeRepoScanner, FakeSessionRegistry,
        ServiceFixture,
    };

    struct CommandFixture;

    impl CommandFixture {
        fn session() -> Session {
            ServiceFixture::session("fix-auth")
        }

        fn live_with_delete_request() -> ServiceFixture {
            let session = Self::session();
            let request = ReturnRequest::Delete(session.tmux_name().to_string());
            Self::live().with_multiplexer(
                FakeMultiplexer::new()
                    .with_live(ServiceFixture::waiting(&session))
                    .with_return_request(request),
            )
        }

        fn live() -> ServiceFixture {
            let session = Self::session();
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&session))
                .with_workspace(ServiceFixture::workspace_for(&[&session]))
                .with_host(ServiceFixture::host_for(&[&session]))
                .with_multiplexer(
                    FakeMultiplexer::new().with_live(ServiceFixture::waiting(&session)),
                )
        }

        fn repo(name: &str, path: &str) -> Repo {
            Repo::discovered(RepoName::parse(name).unwrap(), Path::new(path))
        }

        fn renamed_by_hand() -> ServiceFixture {
            let entry = Self::session();
            let renamed = crate::domain::SessionObservation::new(
                crate::domain::TmuxAddress::new("ringleader-fix-login", entry.socket_path()),
                ServiceFixture::CREATED_AT,
            )
            .with_agent_pane(crate::domain::AgentPane::live(0))
            .with_metadata(entry.metadata().with_branch("fix-login"));
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&entry))
                .with_multiplexer(FakeMultiplexer::new().with_live(renamed))
        }

        fn stopped(names: &[&str], with_worktrees: &[&str]) -> ServiceFixture {
            let sessions: Vec<Session> = names
                .iter()
                .map(|name| ServiceFixture::session(name))
                .collect();
            let present: Vec<&Session> = sessions
                .iter()
                .filter(|session| with_worktrees.contains(&session.name().as_str()))
                .collect();
            ServiceFixture::new()
                .with_sessions(&sessions)
                .with_workspace(ServiceFixture::workspace_for(&present))
                .with_host(ServiceFixture::host_for(&present))
        }

        fn broken_config() -> ConfigError {
            ConfigError::Parse {
                path: PathBuf::from(ServiceFixture::REPO),
                detail: "expected `=`".to_string(),
            }
        }
    }

    #[test]
    fn send_submits_the_prompt_to_the_agent_of_a_waiting_session() {
        let harness = CliHarness::new(CommandFixture::live());

        let exit = harness.run(&["send", "fix-auth", "review the diff\nthen commit"]);

        assert_eq!(exit, CliExit::Code(0));
        assert_eq!(
            harness
                .fixture()
                .multiplexer()
                .submitted_when_ready("ringleader-fix-auth"),
            ["review the diff\nthen commit"]
        );
        assert!(harness.stdout().contains("sent the prompt to fix-auth"));
    }

    #[test]
    fn send_to_an_unknown_session_exits_two() {
        let harness = CliHarness::new(ServiceFixture::new());

        let exit = harness.run(&["send", "fix-auth", "review the diff"]);

        assert_eq!(exit, CliExit::Code(CliExit::NOT_FOUND));
    }

    #[test]
    fn send_to_a_stopped_session_exits_three_and_submits_nothing() {
        let harness = CliHarness::new(CommandFixture::stopped(&["fix-auth"], &["fix-auth"]));

        let exit = harness.run(&["send", "fix-auth", "review the diff"]);

        assert_eq!(exit, CliExit::Code(CliExit::CONFLICT));
        assert!(
            harness
                .stderr()
                .contains("restart it before sending it a prompt")
        );
        assert!(
            harness
                .fixture()
                .multiplexer()
                .submitted_when_ready("ringleader-fix-auth")
                .is_empty()
        );
    }

    #[test]
    fn send_of_a_blank_prompt_exits_one_and_submits_nothing() {
        let harness = CliHarness::new(CommandFixture::live());

        let exit = harness.run(&["send", "fix-auth", "  \n "]);

        assert_eq!(exit, CliExit::Code(CliExit::FAILURE));
        assert!(
            harness
                .fixture()
                .multiplexer()
                .submitted_when_ready("ringleader-fix-auth")
                .is_empty()
        );
    }

    #[test]
    fn send_to_an_agent_that_never_reaches_its_prompt_exits_three_and_names_the_draft() {
        let session = CommandFixture::session();
        let multiplexer = FakeMultiplexer::new()
            .with_live(ServiceFixture::waiting(&session))
            .failing(
                "submit_when_ready",
                MultiplexerError::AgentNotReady {
                    tmux_name: session.tmux_name().to_string(),
                },
            );
        let harness = CliHarness::new(
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&session))
                .with_multiplexer(multiplexer),
        );

        let exit = harness.run(&["send", "fix-auth", "review the diff"]);

        assert_eq!(exit, CliExit::Code(CliExit::CONFLICT));
        assert!(
            harness
                .stderr()
                .contains("send or clear its draft, or leave copy mode before sending")
        );
    }

    #[test]
    fn send_takes_no_lock() {
        let harness = CliHarness::new(CommandFixture::live());

        harness.run(&["send", "fix-auth", "review the diff"]);

        assert!(harness.lock_acquisitions().is_empty());
    }

    #[test]
    fn send_accepts_a_prompt_that_starts_with_a_dash() {
        let harness = CliHarness::new(CommandFixture::live());

        let exit = harness.run(&["send", "fix-auth", "--help me find the bug"]);

        assert_eq!(exit, CliExit::Code(0));
    }

    #[test]
    fn list_prints_one_line_per_session() {
        let harness = CliHarness::new(CommandFixture::live());

        assert_eq!(harness.run(&["list"]), CliExit::Code(0));
        assert!(harness.stdout().starts_with("waiting   fix-auth"));
    }

    #[test]
    fn list_json_uses_stable_state_strings_and_a_null_diff_for_unmanaged_rows() {
        let stranger = ServiceFixture::session("mystery");
        let fixture = CommandFixture::live();
        let fixture = fixture.with_multiplexer(
            FakeMultiplexer::new()
                .with_live(ServiceFixture::waiting(&CommandFixture::session()))
                .with_live(ServiceFixture::waiting(&stranger).without_metadata()),
        );
        let harness = CliHarness::new(fixture);

        harness.run(&["list", "--json"]);

        let rows: serde_json::Value = serde_json::from_str(&harness.stdout()).unwrap();
        assert_eq!(rows[0]["state"], "waiting");
        assert_eq!(rows[0]["diff"]["files"], 0);
        assert_eq!(rows[1]["state"], "unmanaged");
        assert!(rows[1]["diff"].is_null());
    }

    #[test]
    fn list_never_writes_the_registry() {
        let adoptable = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new().with_multiplexer(
            FakeMultiplexer::new().with_live(ServiceFixture::waiting(&adoptable)),
        );
        let harness = CliHarness::new(fixture);

        harness.run(&["list"]);

        assert_eq!(harness.fixture().registry().save_count(), 0);
    }

    #[test]
    fn list_into_a_broken_pipe_stops_and_exits_zero() {
        let harness =
            CliHarness::new(CommandFixture::live()).with_stdout(CapturedWriter::broken_pipe());

        assert_eq!(harness.run(&["list"]), CliExit::Code(0));
    }

    #[test]
    fn a_corrupt_registry_makes_list_exit_one_and_changes_nothing() {
        let registry = FakeSessionRegistry::empty().failing_load(RegistryError::Corrupt {
            path: PathBuf::from("/state/ringleader/sessions.json"),
            detail: "expected value".to_string(),
        });
        let harness = CliHarness::new(ServiceFixture::new().with_registry(registry));

        assert_eq!(harness.run(&["list"]), CliExit::Code(1));
        assert!(!harness.fixture().registry().was_moved_aside());
    }

    #[test]
    fn a_changing_command_moves_a_corrupt_registry_aside() {
        let registry = FakeSessionRegistry::empty().failing_load(RegistryError::Corrupt {
            path: PathBuf::from("/state/ringleader/sessions.json"),
            detail: "expected value".to_string(),
        });
        let harness = CliHarness::new(ServiceFixture::new().with_registry(registry));

        harness.run(&["restart", "anything"]);

        assert!(harness.fixture().registry().was_moved_aside());
    }

    #[test]
    fn list_recreates_a_lost_server_socket_and_says_so() {
        let harness = CliHarness::new(
            ServiceFixture::new().with_multiplexer(FakeMultiplexer::new().with_lost_socket()),
        );

        assert_eq!(harness.run(&["list"]), CliExit::Code(0));
        assert!(
            harness
                .stderr()
                .contains("recreated the socket of Ringleader's tmux server")
        );
    }

    #[test]
    fn an_unparsable_config_makes_list_and_attach_exit_one_without_tmux() {
        let harness = CliHarness::new(
            CommandFixture::live().with_broken_config(CommandFixture::broken_config()),
        );

        assert_eq!(harness.run(&["list"]), CliExit::Code(1));
        assert_eq!(harness.run(&["attach", "fix-auth"]), CliExit::Code(1));
        assert!(harness.fixture().multiplexer().calls().is_empty());
    }

    #[test]
    fn new_creates_a_session_in_a_repo_named_by_the_scan() {
        let harness = CliHarness::new(ServiceFixture::new());

        let exit = harness.run(&["new", "--name", "Fix Auth", "--repo", "example-project"]);

        assert_eq!(exit, CliExit::Code(0));
        assert!(harness.stdout().starts_with("created fix-auth"));
    }

    #[test]
    fn new_with_main_checkout_creates_the_session_in_the_repo_on_its_checked_out_branch() {
        let fixture =
            ServiceFixture::new().with_workspace(ServiceFixture::main_checkout_workspace());
        let harness = CliHarness::new(fixture);

        let exit = harness.run(&[
            "new",
            "--name",
            "fix-auth",
            "--repo",
            "example-project",
            "--main-checkout",
        ]);

        assert_eq!(exit, CliExit::Code(0));
        assert!(harness.stdout().starts_with(&format!(
            "created fix-auth in {} on branch main",
            ServiceFixture::REPO
        )));
    }

    #[test]
    fn deleting_a_session_in_the_main_checkout_prints_what_it_kept_without_a_loss_warning() {
        let session = ServiceFixture::main_checkout_session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_workspace(ServiceFixture::main_checkout_workspace())
            .with_host(ServiceFixture::host_for(&[&session]))
            .with_multiplexer(FakeMultiplexer::new().with_live(ServiceFixture::waiting(&session)));
        let harness = CliHarness::new(fixture);

        let exit = harness.run(&["delete", "fix-auth"]);

        assert_eq!(exit, CliExit::Code(0));
        assert!(harness.stdout().contains("kept the main checkout"));
        assert!(harness.stderr().is_empty(), "{}", harness.stderr());
    }

    #[test]
    fn new_with_a_not_allowed_envrc_warns_on_stderr_and_exits_zero() {
        let fixture = ServiceFixture::new()
            .with_directory_environment(ServiceFixture::not_allowed_envrc("fix-auth"));
        let harness = CliHarness::new(fixture);

        let exit = harness.run(&["new", "--name", "fix-auth", "--repo", "example-project"]);

        assert_eq!(exit, CliExit::Code(0));
        assert!(
            harness.stderr().contains("is not allowed"),
            "{}",
            harness.stderr()
        );
        assert!(harness.stdout().starts_with("created fix-auth"));
    }

    #[test]
    fn restart_with_a_not_allowed_envrc_warns_on_stderr_and_exits_zero() {
        let session = CommandFixture::session();
        let fixture = CommandFixture::live()
            .with_multiplexer(FakeMultiplexer::new().with_live(ServiceFixture::exited(&session)))
            .with_directory_environment(ServiceFixture::not_allowed_envrc("fix-auth"));
        let harness = CliHarness::new(fixture);

        let exit = harness.run(&["restart", "fix-auth"]);

        assert_eq!(exit, CliExit::Code(0));
        assert!(
            harness.stderr().contains("is not allowed"),
            "{}",
            harness.stderr()
        );
        assert_eq!(harness.stdout(), "restarted fix-auth\n");
    }

    #[test]
    fn new_reapplies_a_drifted_server_config_before_creating_the_session() {
        let fixture = ServiceFixture::new()
            .with_multiplexer(FakeMultiplexer::new().with_drifted_config("M-x"));
        let harness = CliHarness::new(fixture);

        harness.run(&["new", "--name", "fix-auth", "--repo", "example-project"]);

        let calls = harness.fixture().multiplexer().calls();
        let position = |prefix: &str| calls.iter().position(|call| call.starts_with(prefix));
        assert!(position("apply_config").unwrap() < position("create_session").unwrap());
    }

    #[test]
    fn new_without_agents_exits_one_naming_the_config_path() {
        let fixture =
            ServiceFixture::new().with_config(ServiceFixture::config().with_agents(vec![]));
        let harness = CliHarness::new(fixture);

        let exit = harness.run(&["new", "--name", "x", "--repo", "example-project"]);

        assert_eq!(exit, CliExit::Code(1));
        assert!(
            harness
                .stderr()
                .contains("no agents configured in /config/ringleader/config.toml")
        );
    }

    #[test]
    fn new_with_a_prompt_for_an_agent_without_a_placeholder_exits_one_before_git() {
        let agent = crate::domain::Agent::new("claude", "claude");
        let fixture =
            ServiceFixture::new().with_config(ServiceFixture::config().with_agents(vec![agent]));
        let harness = CliHarness::new(fixture);

        let exit = harness.run(&[
            "new",
            "--name",
            "x",
            "--repo",
            "example-project",
            "--prompt",
            "go",
        ]);

        assert_eq!(exit, CliExit::Code(1));
        assert!(
            harness
                .fixture()
                .workspace()
                .state_changing_calls()
                .is_empty()
        );
    }

    #[test]
    fn new_with_plan_launches_the_agent_with_its_plan_args() {
        let harness = CliHarness::new(ServiceFixture::new());

        let exit = harness.run(&[
            "new",
            "--name",
            "fix-auth",
            "--repo",
            "example-project",
            "--plan",
        ]);

        let launch = harness.fixture().multiplexer().launches().pop().unwrap();
        assert_eq!(exit, CliExit::Code(0));
        assert_eq!(launch.argv(), ["claude", "--permission-mode", "plan"]);
    }

    #[test]
    fn new_without_plan_launches_the_agent_in_build_mode() {
        let harness = CliHarness::new(ServiceFixture::new());

        harness.run(&["new", "--name", "fix-auth", "--repo", "example-project"]);

        let launch = harness.fixture().multiplexer().launches().pop().unwrap();
        assert_eq!(launch.argv(), ["claude"]);
    }

    #[test]
    fn new_with_plan_for_an_agent_without_plan_args_exits_one_before_git() {
        let agent = crate::domain::Agent::new("claude", "claude");
        let fixture =
            ServiceFixture::new().with_config(ServiceFixture::config().with_agents(vec![agent]));
        let harness = CliHarness::new(fixture);

        let exit = harness.run(&[
            "new",
            "--name",
            "fix-auth",
            "--repo",
            "example-project",
            "--plan",
        ]);

        assert_eq!(exit, CliExit::Code(1));
        assert!(harness.stderr().contains("it has no plan_args"));
        assert!(
            harness
                .fixture()
                .workspace()
                .state_changing_calls()
                .is_empty()
        );
    }

    #[test]
    fn a_repo_name_matching_two_repos_exits_one_and_lists_both_paths() {
        let scanner = FakeRepoScanner::finding(vec![
            CommandFixture::repo("example-project", "/work/one/example-project"),
            CommandFixture::repo("example-project", "/work/two/example-project"),
        ]);
        let harness = CliHarness::new(ServiceFixture::new().with_scanner(scanner));

        let exit = harness.run(&["new", "--name", "x", "--repo", "example-project"]);

        assert_eq!(exit, CliExit::Code(1));
        assert!(
            harness
                .stderr()
                .contains("/work/one/example-project, /work/two/example-project")
        );
    }

    #[test]
    fn a_repo_value_naming_a_repo_and_a_relative_directory_picks_the_repo() {
        let scanner = FakeRepoScanner::finding(vec![CommandFixture::repo("tools", "/work/tools")]);
        let fixture = ServiceFixture::new().with_scanner(scanner).with_host(
            ServiceFixture::host_for(&[]).with_directory(Path::new("/repos/example-project/tools")),
        );
        let harness = CliHarness::new(fixture);

        harness.run(&["new", "--name", "x", "--repo", "tools"]);

        let spec = harness.fixture().multiplexer().specs().pop().unwrap();
        assert_eq!(spec.metadata().repo_path(), Path::new("/work/tools"));
    }

    #[test]
    fn a_path_repo_value_is_validated_and_matched_to_its_explicit_override() {
        let pinned = Repo::explicit(
            RepoName::parse("pinned").unwrap(),
            Path::new(ServiceFixture::REPO),
            None,
        );
        let fixture = ServiceFixture::new()
            .with_config(ServiceFixture::config().with_repos(vec![pinned]))
            .with_scanner(FakeRepoScanner::finding(Vec::new()));
        let harness = CliHarness::new(fixture);

        harness.run(&["new", "--name", "x", "--repo", "."]);

        let spec = harness.fixture().multiplexer().specs().pop().unwrap();
        assert_eq!(spec.metadata().repo_name().as_str(), "pinned");
    }

    #[test]
    fn an_interrupt_while_the_scan_blocks_ends_new_without_creating() {
        let fixture =
            ServiceFixture::new().with_scanner(FakeRepoScanner::finding(Vec::new()).blocked());
        let harness = CliHarness::new(fixture);
        harness.signals().raise(SIGINT);

        let exit = harness.run(&["new", "--name", "x", "--repo", "example-project"]);

        harness.fixture().scanner().release();
        assert!(matches!(exit, CliExit::Interrupted { signal: SIGINT, .. }));
        assert!(harness.fixture().multiplexer().specs().is_empty());
    }

    #[test]
    fn a_signal_raised_before_acting_ends_the_command_without_running_it() {
        let harness = CliHarness::new(CommandFixture::live());
        harness.signals().raise(SIGINT);

        let exit = harness.run(&["delete", "fix-auth", "--force"]);

        assert!(matches!(exit, CliExit::Interrupted { signal: SIGINT, .. }));
        assert_eq!(
            harness.fixture().multiplexer().live_names(),
            ["ringleader-fix-auth"]
        );
    }

    #[test]
    fn a_hangup_during_the_use_case_lets_it_finish_and_reports_the_signal() {
        let fixture = CommandFixture::live();
        let multiplexer = fixture.shared_multiplexer();
        let harness = CliHarness::new(fixture);
        let signals = harness.signals().clone();
        multiplexer.add_call_hook("kill_session", move || signals.raise(SIGHUP));

        let exit = harness.run(&["delete", "fix-auth", "--force"]);

        assert_eq!(
            exit,
            CliExit::Interrupted {
                signal: SIGHUP,
                code: 0
            }
        );
        assert!(harness.fixture().registry().sessions().is_empty());
        assert!(harness.stderr().contains("finished before stopping"));
    }

    #[test]
    fn a_delete_that_would_lose_work_exits_three_and_names_the_loss() {
        let session = CommandFixture::session();
        let fixture = CommandFixture::live().with_workspace(
            ServiceFixture::workspace_for(&[&session])
                .with_dirty(session.worktree_path(), DirtyState::present(2, Vec::new())),
        );
        let harness = CliHarness::new(fixture);

        let exit = harness.run(&["delete", "fix-auth"]);

        assert_eq!(exit, CliExit::Code(3));
        assert!(
            harness
                .stderr()
                .contains("2 uncommitted files will be lost")
        );
        assert!(harness.stderr().contains(".env"));
    }

    #[test]
    fn a_refused_delete_into_a_broken_pipe_still_exits_three() {
        let session = CommandFixture::session();
        let fixture = CommandFixture::live().with_workspace(
            ServiceFixture::workspace_for(&[&session])
                .with_dirty(session.worktree_path(), DirtyState::present(2, Vec::new())),
        );
        let harness = CliHarness::new(fixture).with_stdout(CapturedWriter::broken_pipe());

        assert_eq!(harness.run(&["delete", "fix-auth"]), CliExit::Code(3));
    }

    #[test]
    fn a_forced_delete_warns_on_stderr_and_prints_the_tip() {
        let session = CommandFixture::session();
        let fixture = CommandFixture::live().with_workspace(
            ServiceFixture::workspace_for(&[&session])
                .with_dirty(session.worktree_path(), DirtyState::present(2, Vec::new())),
        );
        let harness = CliHarness::new(fixture);

        let exit = harness.run(&["delete", "fix-auth", "--force"]);

        assert_eq!(exit, CliExit::Code(0));
        assert!(
            harness
                .stderr()
                .contains("warning: 2 uncommitted files will be lost")
        );
        assert!(harness.stdout().contains(ServiceFixture::TIP));
    }

    #[test]
    fn a_delete_that_leaves_a_stray_session_warns_on_stderr() {
        let recorded = Path::new(ServiceFixture::OTHER_RUNTIME_SOCKET);
        let session = CommandFixture::session().on_socket(recorded);
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_workspace(ServiceFixture::workspace_for(&[&session]))
            .with_host(ServiceFixture::host_for(&[&session]).with_tmux_server_on(recorded));
        let harness = CliHarness::new(fixture);

        let exit = harness.run(&["delete", "fix-auth", "--force"]);

        assert_eq!(exit, CliExit::Code(0));
        assert!(harness.stderr().contains(&format!(
            "warning: the tmux server at {} still runs",
            recorded.display()
        )));
        assert!(!harness.stdout().contains("kill-session"));
    }

    #[test]
    fn delete_forget_reports_what_it_left_on_disk() {
        let harness = CliHarness::new(CommandFixture::live());

        harness.run(&["delete", "fix-auth", "--forget", "--force"]);

        assert!(harness.stdout().contains("left the worktree"));
    }

    #[test]
    fn unknown_names_exit_two_and_unmanaged_sessions_exit_three() {
        let stranger = ServiceFixture::session("mystery");
        let fixture = ServiceFixture::new().with_multiplexer(
            FakeMultiplexer::new().with_live(ServiceFixture::waiting(&stranger).without_metadata()),
        );
        let harness = CliHarness::new(fixture);

        for command in [["restart", "nope"], ["delete", "nope"]] {
            assert_eq!(harness.run(&command), CliExit::Code(2));
        }
        assert_eq!(harness.run(&["rename", "nope", "other"]), CliExit::Code(2));
        for command in [["restart", "mystery"], ["delete", "mystery"]] {
            assert_eq!(harness.run(&command), CliExit::Code(3));
        }
        assert_eq!(
            harness.run(&["rename", "mystery", "other"]),
            CliExit::Code(3)
        );
    }

    #[test]
    fn restart_all_prints_each_restarted_session_and_exits_zero() {
        let harness = CliHarness::new(CommandFixture::stopped(
            &["alpha", "bravo"],
            &["alpha", "bravo"],
        ));

        let exit = harness.run(&["restart", "--all"]);

        assert_eq!(exit, CliExit::Code(0));
        assert_eq!(harness.stdout(), "restarted alpha\nrestarted bravo\n");
    }

    #[test]
    fn restart_all_names_each_failure_and_exits_with_its_code() {
        let harness = CliHarness::new(CommandFixture::stopped(&["alpha", "bravo"], &["bravo"]));

        let exit = harness.run(&["restart", "--all"]);

        assert_eq!(exit, CliExit::Code(1));
        assert!(
            harness.stderr().contains("ringleader: alpha: worktree "),
            "{}",
            harness.stderr()
        );
        assert_eq!(harness.stdout(), "restarted bravo\n");
    }

    #[test]
    fn restart_all_without_an_exited_or_stopped_session_says_so_and_exits_zero() {
        let harness = CliHarness::new(CommandFixture::live());

        let exit = harness.run(&["restart", "--all"]);

        assert_eq!(exit, CliExit::Code(0));
        assert_eq!(
            harness.stderr(),
            "no exited or stopped session to restart\n"
        );
    }

    #[test]
    fn restart_all_takes_the_write_lock_once() {
        let harness = CliHarness::new(CommandFixture::stopped(
            &["alpha", "bravo"],
            &["alpha", "bravo"],
        ));

        harness.run(&["restart", "--all"]);

        assert_eq!(harness.lock_acquisitions(), ["try"]);
    }

    #[test]
    fn restart_of_a_waiting_session_exits_three() {
        let harness = CliHarness::new(CommandFixture::live());

        assert_eq!(harness.run(&["restart", "fix-auth"]), CliExit::Code(3));
    }

    #[test]
    fn restart_of_a_session_whose_agent_is_gone_exits_one() {
        let session = CommandFixture::session();
        let fixture = CommandFixture::live()
            .with_config(ServiceFixture::config().with_agents(vec![]))
            .with_multiplexer(FakeMultiplexer::new().with_live(ServiceFixture::exited(&session)));
        let harness = CliHarness::new(fixture);

        assert_eq!(harness.run(&["restart", "fix-auth"]), CliExit::Code(1));
    }

    #[test]
    fn stop_prints_the_stopped_session() {
        let harness = CliHarness::new(CommandFixture::live());

        assert_eq!(harness.run(&["stop", "fix-auth"]), CliExit::Code(0));
        assert_eq!(harness.stdout(), "stopped fix-auth\n");
    }

    #[test]
    fn stop_of_a_stopped_session_exits_three() {
        let harness =
            CliHarness::new(CommandFixture::live().with_multiplexer(FakeMultiplexer::new()));

        assert_eq!(harness.run(&["stop", "fix-auth"]), CliExit::Code(3));
    }

    #[test]
    fn stop_with_a_repo_prints_each_stopped_session_of_that_repo() {
        let harness = CliHarness::new(CommandFixture::live());

        let exit = harness.run(&["stop", "--repo", "example-project"]);

        assert_eq!(exit, CliExit::Code(0));
        assert_eq!(harness.stdout(), "stopped fix-auth\n");
    }

    #[test]
    fn stop_of_several_sessions_keeps_going_past_a_refusal_and_exits_with_its_code() {
        let harness = CliHarness::new(CommandFixture::live());

        let exit = harness.run(&["stop", "missing", "fix-auth"]);

        assert_eq!(exit, CliExit::Code(CliExit::NOT_FOUND));
        assert!(harness.stderr().starts_with("ringleader: missing: "));
        assert_eq!(harness.stdout(), "stopped fix-auth\n");
    }

    #[test]
    fn stop_with_a_repo_notes_a_skipped_working_session() {
        let session = CommandFixture::session();
        let harness =
            CliHarness::new(CommandFixture::live().with_multiplexer(
                FakeMultiplexer::new().with_live(ServiceFixture::working(&session)),
            ));

        let exit = harness.run(&["stop", "--repo", "example-project"]);

        assert_eq!(exit, CliExit::Code(0));
        assert!(
            harness.stderr().contains("skipped fix-auth, working"),
            "{}",
            harness.stderr()
        );
    }

    #[test]
    fn stop_with_a_repo_exits_with_the_code_of_a_refused_stop() {
        let harness = CliHarness::new(
            CommandFixture::live().with_multiplexer(
                FakeMultiplexer::new()
                    .with_live(ServiceFixture::waiting(&CommandFixture::session()))
                    .with_caller("ringleader-fix-auth"),
            ),
        );

        let exit = harness.run(&["stop", "--repo", "example-project"]);

        assert_eq!(exit, CliExit::Code(3));
        assert!(
            harness.stderr().contains("fix-auth: "),
            "{}",
            harness.stderr()
        );
    }

    #[test]
    fn restart_of_several_sessions_restarts_only_the_named_ones() {
        let harness = CliHarness::new(CommandFixture::stopped(
            &["alpha", "bravo", "charlie"],
            &["alpha", "bravo", "charlie"],
        ));

        let exit = harness.run(&["restart", "alpha", "charlie"]);

        assert_eq!(exit, CliExit::Code(0));
        assert_eq!(harness.stdout(), "restarted alpha\nrestarted charlie\n");
    }

    #[test]
    fn delete_of_several_sessions_deletes_each() {
        let harness = CliHarness::new(CommandFixture::stopped(
            &["alpha", "bravo"],
            &["alpha", "bravo"],
        ));

        let exit = harness.run(&["delete", "alpha", "bravo"]);

        assert_eq!(exit, CliExit::Code(0));
        assert!(harness.fixture().registry().sessions().is_empty());
    }

    #[test]
    fn rename_prints_the_new_name() {
        let harness = CliHarness::new(CommandFixture::live());

        assert_eq!(
            harness.run(&["rename", "fix-auth", "fix-login"]),
            CliExit::Code(0)
        );
        assert_eq!(harness.stdout(), "renamed fix-auth to fix-login\n");
    }

    #[test]
    fn a_changing_command_records_a_pending_rekey_before_it_acts() {
        let harness = CliHarness::new(CommandFixture::renamed_by_hand());

        harness.run(&["restart", "unrelated"]);

        assert_eq!(
            harness.fixture().registry().sessions()[0].name().as_str(),
            "fix-login"
        );
    }

    #[test]
    fn new_records_a_pending_rekey_even_when_its_repo_is_unknown() {
        let harness = CliHarness::new(CommandFixture::renamed_by_hand());

        harness.run(&["new", "--name", "other", "--repo", "no-such-repo"]);

        assert_eq!(
            harness.fixture().registry().sessions()[0].name().as_str(),
            "fix-login"
        );
    }

    #[test]
    fn attach_targets_the_agent_pane_as_the_tui_does() {
        let harness = CliHarness::new(CommandFixture::live());

        assert_eq!(harness.run(&["attach", "fix-auth"]), CliExit::Code(0));
        assert_eq!(
            harness.fixture().multiplexer().attached(),
            [AttachTarget::Agent("ringleader-fix-auth".to_string())]
        );
    }

    #[test]
    fn attach_names_the_delete_command_for_a_delete_requested_from_the_session_menu() {
        let harness = CliHarness::new(CommandFixture::live_with_delete_request());

        harness.run(&["attach", "fix-auth"]);

        assert_eq!(harness.stdout(), "run ringleader delete fix-auth\n");
    }

    #[test]
    fn attach_clears_the_return_request_it_finds() {
        let harness = CliHarness::new(CommandFixture::live_with_delete_request());

        harness.run(&["attach", "fix-auth"]);

        assert_eq!(
            harness.fixture().multiplexer().recorded_return_request(),
            None
        );
    }

    #[test]
    fn attach_from_a_pane_of_the_own_server_exits_three() {
        let session = CommandFixture::session();
        let fixture = CommandFixture::live().with_multiplexer(
            FakeMultiplexer::new()
                .with_live(ServiceFixture::waiting(&session))
                .called_from_own_pane(),
        );
        let harness = CliHarness::new(fixture);

        assert_eq!(harness.run(&["attach", "fix-auth"]), CliExit::Code(3));
        assert!(
            harness
                .stderr()
                .contains("run Ringleader from another terminal")
        );
    }
}
