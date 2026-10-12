use std::path::Path;
use std::sync::Arc;

use super::diagnostic_paths::DiagnosticPaths;
use super::doctor_report::DoctorReport;
use super::ports::{
    Clock, ConfigError, HostProbe, LeftoverBinding, LeftoverBindings, Multiplexer, SessionRegistry,
    Workspace,
};
use super::registry_audit::RegistryAudit;
use super::service_ports::ServicePorts;
use super::session_reconciler::SessionReconciler;
use super::tool_version::ToolVersion;
use super::use_checks::UnresolvedCommand;
use crate::domain::{Agent, Config};

pub struct Doctor {
    audit: RegistryAudit,
    workspace: Arc<dyn Workspace>,
    multiplexer: Arc<dyn Multiplexer>,
    host: Arc<dyn HostProbe>,
    registry: Arc<dyn SessionRegistry>,
    leftover_bindings: Arc<dyn LeftoverBindings>,
    clock: Arc<dyn Clock>,
    paths: DiagnosticPaths,
}

impl Doctor {
    const MINIMUM_TMUX: (u32, u32) = (3, 2);
    const MINIMUM_GIT: (u32, u32) = (2, 42);
    const NIX_STORE: &'static str = "/nix/store";

    pub fn new(ports: &ServicePorts, paths: DiagnosticPaths) -> Self {
        Self {
            audit: RegistryAudit::new(ports),
            workspace: ports.repos().workspace(),
            multiplexer: ports.tmux().multiplexer(),
            host: ports.repos().host(),
            registry: ports.state().registry(),
            leftover_bindings: ports.tmux().leftover_bindings(),
            clock: ports.state().clock(),
            paths,
        }
    }

    pub fn examine(
        &self,
        config: &Config,
        config_error: Option<&ConfigError>,
        config_path: &Path,
    ) -> DoctorReport {
        let mut report = DoctorReport::default();
        self.check_tmux(&mut report);
        self.check_git(&mut report);
        let valid_config = match config_error {
            Some(error) => {
                Self::report_config_error(error, &mut report);
                None
            }
            None => {
                self.check_config(config, config_path, &mut report);
                Some(config)
            }
        };
        self.check_config_writable(config_path, &mut report);
        self.check_writable("state directory", self.paths.state_directory(), &mut report);
        self.check_registry(config, valid_config, &mut report);
        self.check_leftover_bindings(&mut report);
        report
    }

    fn check_tmux(&self, report: &mut DoctorReport) {
        let program = self.paths.tmux_program();
        report.info(format!("tmux program: {program}"));
        match self.multiplexer.version() {
            Ok(version) => Self::check_version("tmux client", &version, Self::MINIMUM_TMUX, report),
            Err(error) => report.problem(format!("tmux {program} is not usable: {error}")),
        }
        match self.multiplexer.server_version() {
            Ok(Some(version)) => {
                Self::check_version("tmux server", &version, Self::MINIMUM_TMUX, report)
            }
            Ok(None) => report.info("no tmux server is running"),
            Err(error) => report.problem(format!("cannot read the tmux server version: {error}")),
        }
        if let Ok(Some(true)) = self.multiplexer.server_exit_unattached() {
            report.problem("the running tmux server has exit-unattached on, so creating a session fails; turn it off");
        }
    }

    fn check_leftover_bindings(&self, report: &mut DoctorReport) {
        let path = self.leftover_bindings.path();
        match self.leftover_bindings.read() {
            Ok(bindings) => {
                for binding in bindings {
                    report.warning(Self::leftover_advice(&path, &binding));
                }
            }
            Err(error) => report.warning(format!("leftover Ctrl-Q bindings: {error}")),
        }
    }

    fn leftover_advice(path: &Path, binding: &LeftoverBinding) -> String {
        let file = path.display();
        let socket = binding.socket().display();
        let fix = match binding.saved_line() {
            Some(line) => format!(
                "it saved your binding {line} from the tmux server at {socket}. If C-q there still runs Ringleader's binding, put that line in a file and run tmux -S {socket} source-file <file>"
            ),
            None => format!(
                "the tmux server at {socket} had no C-q binding before. If C-q there still runs Ringleader's binding, run tmux -S {socket} unbind-key -T root C-q"
            ),
        };
        format!(
            "{file} is left from an earlier Ringleader that bound C-q on your tmux server: {fix}. Ringleader no longer changes that server, so delete {file} once it is done"
        )
    }

    fn check_git(&self, report: &mut DoctorReport) {
        let program = self.paths.git_program();
        report.info(format!("git program: {program}"));
        match self.workspace.version() {
            Ok(version) => Self::check_version("git", &version, Self::MINIMUM_GIT, report),
            Err(error) => report.problem(format!("git {program} is not usable: {error}")),
        }
    }

    fn check_version(label: &str, version: &str, minimum: (u32, u32), report: &mut DoctorReport) {
        match ToolVersion::new(version).numbers() {
            Some(found) if found >= minimum => report.info(format!("{label} {version}")),
            _ => report.problem(format!(
                "{label} {version} is older than {}.{}",
                minimum.0, minimum.1
            )),
        }
    }

    fn report_config_error(error: &ConfigError, report: &mut DoctorReport) {
        match error {
            ConfigError::Invalid { path, failures } => {
                for failure in failures {
                    report.problem(format!("config {}: {failure}", path.display()));
                }
            }
            other => report.problem(format!("config: {other}")),
        }
    }

    fn check_config(&self, config: &Config, config_path: &Path, report: &mut DoctorReport) {
        for repo in config.repos() {
            if let Err(error) = self.workspace.validate_repo(repo.path()) {
                report.problem(format!("repo {}: {error}", repo.name()));
            }
        }
        if config.agents().is_empty() {
            report.problem(format!("no agents configured in {}", config_path.display()));
        }
        for agent in config.agents() {
            self.check_agent(agent, report);
        }
        let root = config.naming().worktree_root();
        let nearest_existing = root.ancestors().find(|path| self.host.path_exists(path));
        if !nearest_existing.is_some_and(|directory| self.host.is_writable(directory)) {
            report.problem(format!("worktree root {} is not writable", root.display()));
        }
    }

    fn check_agent(&self, agent: &Agent, report: &mut DoctorReport) {
        if self.host.resolve_command(agent.command()).is_none() {
            report.problem(UnresolvedCommand::new(agent).message());
        }
        if !agent.prompt_args().is_empty() && !agent.accepts_prompt() {
            report.problem(format!(
                "agent {} has prompt_args without {}, so it cannot take a prompt",
                agent.name(),
                Agent::PROMPT_PLACEHOLDER
            ));
        }
    }

    fn check_config_writable(&self, config_path: &Path, report: &mut DoctorReport) {
        let canonical = self.host.canonical_or_given(config_path);
        if canonical.starts_with(Self::NIX_STORE) {
            report.warning(format!(
                "config {} is Nix-managed, Ringleader cannot write it",
                config_path.display()
            ));
        } else if self.host.path_exists(config_path) && !self.host.is_writable(config_path) {
            report.warning(format!(
                "config {} is read-only, Ringleader cannot write it",
                config_path.display()
            ));
        }
    }

    fn check_writable(&self, label: &str, directory: &Path, report: &mut DoctorReport) {
        if !self.host.is_writable(directory) {
            report.problem(format!("{label} {} is not writable", directory.display()));
        }
    }

    fn check_registry(
        &self,
        config: &Config,
        valid_config: Option<&Config>,
        report: &mut DoctorReport,
    ) {
        let entries = match self.registry.load() {
            Ok(entries) => entries,
            Err(error) => {
                report.problem(format!("registry: {error}"));
                return;
            }
        };
        let observations = match self.multiplexer.observe_all() {
            Ok(observations) => observations,
            Err(error) => {
                report.problem(format!("cannot observe tmux: {error}"));
                return;
            }
        };
        let now = self.clock.now_epoch_seconds();
        let reconciliation = SessionReconciler::new(config, self.host.as_ref(), now)
            .serving(&self.multiplexer.socket())
            .reconcile(&entries, observations);
        self.audit.audit(&reconciliation, valid_config, report);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::application::ports::LeftoverBindingsError;
    use crate::application::ports::{MultiplexerError, RegistryError};
    use crate::domain::{Repo, RepoName, Session};
    use crate::test_support::{
        FakeLeftoverBindings, FakeMultiplexer, FakeSessionRegistry, FakeWorkspace, ServiceFixture,
    };

    struct DoctorFixture;

    impl DoctorFixture {
        fn session() -> Session {
            ServiceFixture::session("fix-auth")
        }

        fn healthy() -> ServiceFixture {
            let session = Self::session();
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&session))
                .with_workspace(ServiceFixture::workspace_for(&[&session]))
                .with_host(ServiceFixture::host_for(&[&session]))
        }

        fn lines(fixture: &ServiceFixture) -> Vec<String> {
            fixture.unloaded_service().doctor().lines()
        }

        fn has_problem(fixture: &ServiceFixture, fragment: &str) -> bool {
            Self::lines(fixture)
                .iter()
                .any(|line| line.starts_with("problem:") && line.contains(fragment))
        }

        fn has_warning(fixture: &ServiceFixture, fragment: &str) -> bool {
            Self::lines(fixture)
                .iter()
                .any(|line| line.starts_with("warning:") && line.contains(fragment))
        }
    }

    #[test]
    fn an_ordinary_stopped_session_has_no_problem() {
        let report = DoctorFixture::healthy().service().doctor();

        assert!(!report.has_problems(), "{:?}", report.lines());
    }

    #[test]
    fn the_client_and_server_versions_are_both_reported() {
        let fixture = DoctorFixture::healthy()
            .with_multiplexer(FakeMultiplexer::new().with_versions("3.6a", Some("3.7c")));

        let lines = DoctorFixture::lines(&fixture);

        assert!(lines.contains(&"info: tmux client 3.6a".to_string()));
        assert!(lines.contains(&"info: tmux server 3.7c".to_string()));
    }

    #[test]
    fn an_old_tmux_is_a_problem() {
        let fixture = DoctorFixture::healthy()
            .with_multiplexer(FakeMultiplexer::new().with_versions("3.1c", None));

        assert!(DoctorFixture::has_problem(&fixture, "older than 3.2"));
    }

    #[test]
    fn an_unusable_tmux_is_a_problem() {
        let fixture = DoctorFixture::healthy().with_multiplexer(FakeMultiplexer::new().failing(
            "version",
            MultiplexerError::Failed {
                subcommand: "tmux -V".to_string(),
                detail: "No such file or directory".to_string(),
            },
        ));

        assert!(DoctorFixture::has_problem(&fixture, "is not usable"));
    }

    #[test]
    fn a_server_with_exit_unattached_on_is_a_problem() {
        let fixture = DoctorFixture::healthy()
            .with_multiplexer(FakeMultiplexer::new().with_exit_unattached());

        assert!(DoctorFixture::has_problem(&fixture, "exit-unattached"));
    }

    #[test]
    fn an_old_git_is_a_problem() {
        let fixture = DoctorFixture::healthy().with_workspace(
            ServiceFixture::workspace_for(&[&DoctorFixture::session()]).with_version("2.39.2"),
        );

        assert!(DoctorFixture::has_problem(&fixture, "older than 2.42"));
    }

    #[test]
    fn each_config_failure_is_its_own_problem_and_versions_are_still_checked() {
        let fixture = DoctorFixture::healthy().with_broken_config(ConfigError::Invalid {
            path: Path::new("/config/ringleader/config.toml").to_path_buf(),
            failures: vec![
                "tmux_prefix: bad".to_string(),
                "session_size: bad".to_string(),
            ],
        });

        let lines = DoctorFixture::lines(&fixture);

        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains("config /config"))
                .count(),
            2
        );
        assert!(lines.iter().any(|line| line.starts_with("info: tmux")));
        assert!(lines.iter().any(|line| line.starts_with("info: git")));
    }

    #[test]
    fn a_configured_repo_that_git_rejects_is_a_problem() {
        let pinned = Repo::explicit(
            RepoName::parse("pinned").unwrap(),
            Path::new("/gone/pinned"),
            None,
        );
        let fixture = DoctorFixture::healthy()
            .with_config(ServiceFixture::config().with_repos(vec![pinned]))
            .with_workspace(
                ServiceFixture::workspace_for(&[&DoctorFixture::session()])
                    .with_invalid_repo(Path::new("/gone/pinned")),
            );

        assert!(DoctorFixture::has_problem(&fixture, "repo pinned"));
    }

    #[test]
    fn no_agents_is_a_problem() {
        let fixture =
            DoctorFixture::healthy().with_config(ServiceFixture::config().with_agents(vec![]));

        assert!(DoctorFixture::has_problem(&fixture, "no agents configured"));
    }

    #[test]
    fn an_unresolved_command_with_flags_says_to_move_them_into_args() {
        let agent = Agent::new("claude", "claude --model opus");
        let fixture =
            DoctorFixture::healthy().with_config(ServiceFixture::config().with_agents(vec![agent]));

        assert!(DoctorFixture::has_problem(
            &fixture,
            "move the flags into args"
        ));
    }

    #[test]
    fn prompt_args_without_the_placeholder_is_a_problem() {
        let agent = ServiceFixture::agent().with_prompt_args(&["--yes".to_string()]);
        let fixture =
            DoctorFixture::healthy().with_config(ServiceFixture::config().with_agents(vec![agent]));

        assert!(DoctorFixture::has_problem(&fixture, "cannot take a prompt"));
    }

    #[test]
    fn a_read_only_config_is_a_warning() {
        let config_path = Path::new("/config/ringleader/config.toml");
        let session = DoctorFixture::session();
        let fixture = DoctorFixture::healthy().with_host(
            ServiceFixture::host_for(&[&session])
                .with_file(config_path)
                .with_unwritable(config_path),
        );

        assert!(DoctorFixture::has_warning(&fixture, "is read-only"));
    }

    #[test]
    fn an_unwritable_worktree_root_is_a_problem() {
        let session = DoctorFixture::session();
        let fixture = DoctorFixture::healthy().with_host(
            ServiceFixture::host_for(&[&session])
                .with_unwritable(Path::new(ServiceFixture::WORKTREE_ROOT)),
        );

        assert!(DoctorFixture::has_problem(&fixture, "worktree root"));
    }

    #[test]
    fn a_corrupt_registry_is_a_problem() {
        let fixture = DoctorFixture::healthy().with_registry(
            FakeSessionRegistry::empty().failing_load(RegistryError::Corrupt {
                path: Path::new("/state/ringleader/sessions.json").to_path_buf(),
                detail: "expected value".to_string(),
            }),
        );

        assert!(DoctorFixture::has_problem(&fixture, "registry"));
    }

    #[test]
    fn a_missing_worktree_and_branch_are_problems() {
        let fixture = DoctorFixture::healthy()
            .with_workspace(FakeWorkspace::default())
            .with_host(ServiceFixture::host_for(&[]));

        assert!(DoctorFixture::has_problem(&fixture, "is missing"));
        assert!(DoctorFixture::has_problem(
            &fixture,
            "branch ringleader/fix-auth is missing"
        ));
    }

    #[test]
    fn a_leftover_worktree_directory_is_a_problem_with_advice() {
        let fixture =
            DoctorFixture::healthy().with_workspace(FakeWorkspace::default().with_branch(
                Path::new(ServiceFixture::REPO),
                "ringleader/fix-auth",
                ServiceFixture::TIP,
            ));

        assert!(DoctorFixture::has_problem(&fixture, "--forget"));
    }

    #[test]
    fn a_removed_agent_is_a_problem() {
        let other = Agent::new("other", "claude");
        let fixture =
            DoctorFixture::healthy().with_config(ServiceFixture::config().with_agents(vec![other]));

        assert!(DoctorFixture::has_problem(
            &fixture,
            "agent claude is no longer configured"
        ));
    }

    #[test]
    fn a_changed_naming_is_a_warning() {
        let config = ServiceFixture::config().with_naming(crate::domain::NamingScheme::new(
            Path::new(ServiceFixture::WORKTREE_ROOT),
            "ringleader/",
            "rl-",
        ));
        let fixture = DoctorFixture::healthy().with_config(config);

        assert!(DoctorFixture::has_warning(
            &fixture,
            "tmux_prefix ringleader-, which differ from the config"
        ));
    }

    #[test]
    fn an_orphaned_worktree_and_branch_are_warnings() {
        let session = DoctorFixture::session();
        let orphan = ServiceFixture::session("orphan");
        let fixture = DoctorFixture::healthy().with_workspace(
            ServiceFixture::workspace_for(&[&session, &orphan]).with_branch(
                Path::new(ServiceFixture::REPO),
                "ringleader/lonely",
                ServiceFixture::TIP,
            ),
        );

        assert!(DoctorFixture::has_warning(&fixture, "orphaned worktree"));
        assert!(DoctorFixture::has_warning(
            &fixture,
            "orphaned branch ringleader/lonely"
        ));
    }

    #[test]
    fn an_entry_on_an_earlier_server_is_a_warning_with_the_hand_commands() {
        let session = ServiceFixture::on_earlier_server(&DoctorFixture::session());
        let fixture = DoctorFixture::healthy().with_sessions(&[session]);

        let lines = DoctorFixture::lines(&fixture);

        let warning = lines
            .iter()
            .find(|line| line.contains(FakeMultiplexer::EARLIER_SOCKET))
            .unwrap();
        assert!(warning.starts_with("warning:"));
        assert!(warning.contains("no process runs in its worktree"));
        assert!(warning.contains("kill-session -t '=ringleader-fix-auth:'"));
    }

    #[test]
    fn an_entry_on_an_earlier_server_names_the_processes_in_its_worktree() {
        let session = ServiceFixture::on_earlier_server(&DoctorFixture::session());
        let fixture = DoctorFixture::healthy()
            .with_sessions(std::slice::from_ref(&session))
            .with_host(ServiceFixture::with_worktree_process(&session));

        assert!(DoctorFixture::has_warning(
            &fixture,
            "processes still run in its worktree: 4000042 (claude)"
        ));
    }

    #[test]
    fn a_stopped_private_entry_with_processes_in_its_worktree_gets_the_socket_fix() {
        let fixture = DoctorFixture::healthy().with_host(ServiceFixture::with_worktree_process(
            &DoctorFixture::session(),
        ));

        assert!(DoctorFixture::has_warning(&fixture, "send SIGUSR1"));
    }

    #[test]
    fn each_leftover_binding_is_a_warning_with_its_socket_and_hand_command() {
        let fixture =
            DoctorFixture::healthy().with_leftover_bindings(FakeLeftoverBindings::with(vec![
                LeftoverBinding::new(
                    Path::new("/tmp/tmux-1000/default"),
                    Some("bind-key -T root C-q send-keys example"),
                ),
                LeftoverBinding::new(Path::new("/tmp/tmux-1000/work"), None),
            ]));

        let lines = DoctorFixture::lines(&fixture);

        assert!(DoctorFixture::has_warning(
            &fixture,
            "your binding bind-key -T root C-q send-keys example from the tmux server at /tmp/tmux-1000/default"
        ));
        assert!(DoctorFixture::has_warning(
            &fixture,
            "tmux -S /tmp/tmux-1000/work unbind-key -T root C-q"
        ));
        assert!(
            lines
                .iter()
                .filter(|line| line.contains(FakeLeftoverBindings::PATH))
                .all(|line| line.starts_with("warning:"))
        );
        assert!(
            fixture
                .multiplexer()
                .calls()
                .iter()
                .all(|call| !call.contains("/tmp/tmux-1000/work"))
        );
    }

    #[test]
    fn an_unreadable_bindings_file_is_a_warning() {
        let fixture = DoctorFixture::healthy().with_leftover_bindings(
            FakeLeftoverBindings::failing(LeftoverBindingsError::Corrupt {
                path: Path::new(FakeLeftoverBindings::PATH).to_path_buf(),
                detail: "expected value".to_string(),
            }),
        );

        assert!(DoctorFixture::has_warning(&fixture, "is not valid JSON"));
    }

    #[test]
    fn a_tmux_name_held_by_a_session_without_metadata_gets_the_kill_advice() {
        let session = DoctorFixture::session();
        let fixture = DoctorFixture::healthy().with_multiplexer(
            FakeMultiplexer::new().with_live(ServiceFixture::waiting(&session).without_metadata()),
        );

        assert!(DoctorFixture::has_problem(
            &fixture,
            "kill-session -t '=ringleader-fix-auth:'"
        ));
    }
}
