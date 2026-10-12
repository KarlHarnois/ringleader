use std::collections::HashMap;

use super::delete_report::DeleteReport;
use super::list_report::ListReport;
use super::repo_argument::RepoArgument;
use super::{CliSession, CommandStop, DeleteArgs, ListArgs, NewArgs, RenameArgs};
use crate::application::{
    CreateRequest, DeleteConfirmation, DeleteImpact, ServiceError, SessionService,
};
use crate::domain::BaseBranch;

pub struct SessionCommands<'a> {
    service: &'a mut SessionService,
    session: &'a mut CliSession,
}

impl<'a> SessionCommands<'a> {
    pub fn new(service: &'a mut SessionService, session: &'a mut CliSession) -> Self {
        Self { service, session }
    }

    pub fn list(&mut self, args: &ListArgs) -> Result<u8, CommandStop> {
        let rows = self.service.observe(false)?;
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
        self.service.observe(true)?;
        let repo = RepoArgument::new(&args.repo).resolve(self.service, self.session)?;
        let mut request =
            CreateRequest::new(&args.name, repo).with_prompt(args.prompt.as_deref().unwrap_or(""));
        if let Some(agent) = &args.agent {
            request = request.with_agent(agent);
        }
        if let Some(base) = args.base_branch.as_deref().and_then(BaseBranch::new) {
            request = request.with_base_branch(base);
        }
        self.session.stop_if_signaled()?;
        let created = self.service.create_session(&request)?;
        self.session.output().line(&format!(
            "created {} in {} on branch {}",
            created.name(),
            created.worktree_path().display(),
            created.branch()
        ));
        Ok(0)
    }

    pub fn delete(&mut self, args: &DeleteArgs) -> Result<u8, CommandStop> {
        self.service.observe(true)?;
        let confirmation = if args.force {
            DeleteConfirmation::force()
        } else {
            DeleteConfirmation::confirmed(DeleteImpact::default())
        };
        let confirmation = if args.forget {
            confirmation.forgetting()
        } else {
            confirmation
        };
        self.session.stop_if_signaled()?;
        let report = DeleteReport::new(&args.name);
        match self.service.delete_session(&args.name, &confirmation) {
            Ok(outcome) => {
                if outcome.kept().is_none() {
                    for warning in report.warnings(outcome.impact()) {
                        self.session.output().note(&warning);
                    }
                }
                self.session.output().line(&report.success(&outcome));
                Ok(0)
            }
            Err(ServiceError::GuardsChanged { impact, .. }) => {
                for line in report.refusal(&impact) {
                    self.session.output().note(&line);
                }
                Ok(ServiceError::CONFLICT_EXIT)
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn attach(&mut self, name: &str) -> Result<u8, CommandStop> {
        self.service.attach(name)?;
        Ok(0)
    }

    pub fn rename(&mut self, args: &RenameArgs) -> Result<u8, CommandStop> {
        self.service.observe(true)?;
        self.session.stop_if_signaled()?;
        let renamed = self
            .service
            .rename_session(&args.old, &args.new, args.keep_branch)?;
        self.session.output().line(&format!(
            "renamed {} to {}, branch {}",
            args.old,
            renamed.name(),
            renamed.branch()
        ));
        Ok(0)
    }

    pub fn restart(&mut self, name: &str) -> Result<u8, CommandStop> {
        self.service.observe(true)?;
        self.session.stop_if_signaled()?;
        self.service.restart_session(name)?;
        self.session.output().line(&format!("restarted {name}"));
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use signal_hook::consts::{SIGHUP, SIGINT};

    use crate::application::ports::{AttachTarget, ConfigError, DirtyState, RegistryError};
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

        fn broken_config() -> ConfigError {
            ConfigError::Parse {
                path: PathBuf::from(ServiceFixture::REPO),
                detail: "expected `=`".to_string(),
            }
        }
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
        let harness_signals = crate::adapters::signal_flags::SignalFlags::unregistered();
        let raised = harness_signals.clone();
        let session = CommandFixture::session();
        let fixture = CommandFixture::live().with_multiplexer(
            FakeMultiplexer::new()
                .with_live(ServiceFixture::waiting(&session))
                .on_call("kill_session", move || raised.raise(SIGHUP)),
        );
        let harness = CliHarness::new(fixture).with_signals(harness_signals);

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
    fn rename_prints_the_new_name_and_branch() {
        let harness = CliHarness::new(CommandFixture::live());

        assert_eq!(
            harness.run(&["rename", "fix-auth", "fix-login"]),
            CliExit::Code(0)
        );
        assert_eq!(
            harness.stdout(),
            "renamed fix-auth to fix-login, branch ringleader/fix-login\n"
        );
    }

    #[test]
    fn a_changing_command_records_a_pending_rekey_before_it_acts() {
        let entry = CommandFixture::session();
        let renamed = crate::domain::SessionObservation::new(
            crate::domain::TmuxAddress::new("ringleader-fix-login", entry.socket_path()),
            ServiceFixture::CREATED_AT,
        )
        .with_agent_pane(crate::domain::AgentPane::live(0))
        .with_metadata(entry.metadata().with_branch("ringleader/fix-login"));
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&entry))
            .with_multiplexer(FakeMultiplexer::new().with_live(renamed));
        let harness = CliHarness::new(fixture);

        harness.run(&["restart", "unrelated"]);

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
