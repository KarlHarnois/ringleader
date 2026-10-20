use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::ports::{AttachTarget, HostProbe, Multiplexer, TerminalEmulator, WebBrowser};
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use crate::domain::{PullRequest, Session, SessionState, SessionSummary};

pub struct SessionAccess {
    multiplexer: Arc<dyn Multiplexer>,
    host: Arc<dyn HostProbe>,
    terminal_emulator: Arc<dyn TerminalEmulator>,
    web_browser: Arc<dyn WebBrowser>,
    shell: String,
}

impl SessionAccess {
    pub fn new(ports: &ServicePorts, shell: &str) -> Self {
        Self {
            multiplexer: ports.tmux().multiplexer(),
            host: ports.repos().host(),
            terminal_emulator: ports.desktop().terminal_emulator(),
            web_browser: ports.desktop().web_browser(),
            shell: shell.to_string(),
        }
    }

    pub fn refuse_own_pane(&self) -> Result<(), ServiceError> {
        if self.multiplexer.caller_in_own_pane()? {
            return Err(ServiceError::Conflict(
                "this terminal is a pane of Ringleader's own tmux server, run Ringleader from another terminal".to_string(),
            ));
        }
        Ok(())
    }

    pub fn attach(&self, row: &SessionSummary) -> Result<(), ServiceError> {
        if row.state() == SessionState::Stopped {
            return Err(ServiceError::Conflict(format!(
                "session {} is stopped, restart it first",
                row.name()
            )));
        }
        self.refuse_own_pane()?;
        self.multiplexer
            .attach(&AttachTarget::Agent(row.tmux_name().to_string()))?;
        Ok(())
    }

    pub fn open_shell(&self, row: &SessionSummary) -> Result<ShellOutcome, ServiceError> {
        let session = row.session().ok_or_else(|| {
            ServiceError::Invalid(format!(
                "session {} is unmanaged and has no trusted worktree for a shell",
                row.name()
            ))
        })?;
        self.check_worktree(session)?;
        if row.state() == SessionState::Stopped {
            let shell = EphemeralShell::new(&self.shell, session.worktree_path());
            return Ok(ShellOutcome::Ephemeral(shell));
        }
        self.refuse_own_pane()?;
        let window = self.multiplexer.ensure_shell_window(
            session.tmux_name(),
            session.worktree_path(),
            &self.shell,
        )?;
        self.multiplexer.attach(&AttachTarget::Shell(window))?;
        Ok(ShellOutcome::Attached)
    }

    pub fn open_terminal(&self, session: &Session) -> Result<(), ServiceError> {
        self.check_worktree(session)?;
        self.terminal_emulator
            .open_window(session.worktree_path())?;
        Ok(())
    }

    pub fn open_pull_request(&self, pull_request: &PullRequest) -> Result<(), ServiceError> {
        self.web_browser.open(pull_request.url())?;
        Ok(())
    }

    fn check_worktree(&self, session: &Session) -> Result<(), ServiceError> {
        if self.host.path_exists(session.worktree_path()) {
            return Ok(());
        }
        Err(ServiceError::Invalid(format!(
            "worktree {} is missing, run ringleader doctor",
            session.worktree_path().display()
        )))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellOutcome {
    Attached,
    Ephemeral(EphemeralShell),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EphemeralShell {
    program: String,
    worktree: PathBuf,
}

impl EphemeralShell {
    pub fn new(program: &str, worktree: &Path) -> Self {
        Self {
            program: program.to_string(),
            worktree: worktree.to_path_buf(),
        }
    }

    pub fn program(&self) -> &str {
        &self.program
    }

    pub fn worktree(&self) -> &Path {
        &self.worktree
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::SessionService;
    use crate::application::ports::{ConfigError, TerminalEmulatorError, WebBrowserError};
    use crate::domain::PullRequestState;
    use crate::test_support::{
        FakeMultiplexer, FakePullRequests, FakeTerminalEmulator, FakeWebBrowser, ServiceFixture,
    };

    struct AttachFixture;

    impl AttachFixture {
        fn live() -> ServiceFixture {
            let session = ServiceFixture::session("fix-auth");
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&session))
                .with_multiplexer(
                    FakeMultiplexer::new().with_live(ServiceFixture::waiting(&session)),
                )
        }
    }

    struct ShellFixture;

    impl ShellFixture {
        fn live() -> ServiceFixture {
            let session = ServiceFixture::session("fix-auth");
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&session))
                .with_host(ServiceFixture::host_for(&[&session]))
                .with_multiplexer(
                    FakeMultiplexer::new().with_live(ServiceFixture::waiting(&session)),
                )
        }
    }

    struct PullRequestFixture;

    impl PullRequestFixture {
        const NAME: &'static str = "fix-auth";
        const NUMBER: u64 = 142;

        fn with_pull_request() -> ServiceFixture {
            Self::with_pull_request_for(&ServiceFixture::session(Self::NAME))
        }

        fn in_main_checkout_with_pull_request() -> ServiceFixture {
            Self::with_pull_request_for(&ServiceFixture::main_checkout_session(Self::NAME))
        }

        fn with_pull_request_for(session: &Session) -> ServiceFixture {
            let pull_request = ServiceFixture::pull_request(Self::NUMBER, PullRequestState::Open);
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(session))
                .with_pull_requests(FakePullRequests::new().with_pull_request(
                    Path::new(ServiceFixture::REPO),
                    session.branch(),
                    pull_request,
                ))
        }

        fn without_pull_request() -> ServiceFixture {
            ServiceFixture::new().with_sessions(&[ServiceFixture::session(Self::NAME)])
        }

        fn failing_lookup() -> ServiceFixture {
            let pull_requests = FakePullRequests::new();
            pull_requests.fail();
            Self::without_pull_request().with_pull_requests(pull_requests)
        }

        fn refusal(fixture: &ServiceFixture) -> ServiceError {
            fixture.service().open_pull_request(Self::NAME).unwrap_err()
        }

        fn looked_up(fixture: &ServiceFixture) -> SessionService {
            let service = fixture.service();
            service
                .find_pull_requests(&[Self::NAME.to_string()])
                .wait_unless(|| false)
                .unwrap()
                .unwrap();
            service
        }
    }

    struct TerminalFixture;

    impl TerminalFixture {
        fn stopped() -> ServiceFixture {
            let session = ServiceFixture::session("fix-auth");
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&session))
                .with_host(ServiceFixture::host_for(&[&session]))
        }

        fn live() -> ServiceFixture {
            let session = ServiceFixture::session("fix-auth");
            Self::stopped()
                .with_workspace(ServiceFixture::workspace_for(&[&session]))
                .with_multiplexer(
                    FakeMultiplexer::new().with_live(ServiceFixture::waiting(&session)),
                )
        }
    }

    #[test]
    fn attach_targets_the_agent_pane_of_the_session() {
        let fixture = AttachFixture::live();

        fixture.service().attach("fix-auth").unwrap();

        assert_eq!(
            fixture.multiplexer().attached(),
            [AttachTarget::Agent("ringleader-fix-auth".to_string())]
        );
    }

    #[test]
    fn attach_from_a_pane_of_the_own_server_is_a_conflict() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_multiplexer(
                FakeMultiplexer::new()
                    .with_live(ServiceFixture::waiting(&session))
                    .called_from_own_pane(),
            );

        let error = fixture.service().attach("fix-auth").unwrap_err();

        assert!(matches!(error, ServiceError::Conflict(_)));
        assert!(
            error
                .to_string()
                .contains("run Ringleader from another terminal")
        );
        assert!(fixture.multiplexer().attached().is_empty());
    }

    #[test]
    fn attach_refuses_a_stopped_session() {
        let fixture = ServiceFixture::new().with_sessions(&[ServiceFixture::session("fix-auth")]);

        let error = fixture.service().attach("fix-auth").unwrap_err();

        assert!(error.to_string().contains("stopped"));
    }

    #[test]
    fn a_shell_in_a_live_session_attaches_to_its_shell_window() {
        let fixture = ShellFixture::live();

        let outcome = fixture.service().open_shell("fix-auth").unwrap();

        assert_eq!(outcome, ShellOutcome::Attached);
        assert_eq!(
            fixture.multiplexer().attached(),
            [AttachTarget::Shell("@9".to_string())]
        );
    }

    #[test]
    fn a_shell_from_a_pane_of_the_own_server_is_a_conflict() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = ShellFixture::live().with_multiplexer(
            FakeMultiplexer::new()
                .with_live(ServiceFixture::waiting(&session))
                .called_from_own_pane(),
        );

        let error = fixture.service().open_shell("fix-auth").unwrap_err();

        assert!(matches!(error, ServiceError::Conflict(_)));
        assert!(fixture.multiplexer().state_changing_calls().is_empty());
        assert!(fixture.multiplexer().attached().is_empty());
    }

    #[test]
    fn a_shell_in_a_live_session_runs_the_configured_shell() {
        let fixture = ShellFixture::live();

        fixture.service().open_shell("fix-auth").unwrap();

        assert!(
            fixture.multiplexer().calls().contains(
                &"ensure_shell_window ringleader-fix-auth /bin/example-shell".to_string()
            )
        );
    }

    #[test]
    fn a_shell_in_a_stopped_session_is_an_ephemeral_shell_in_its_worktree() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_host(ServiceFixture::host_for(&[&session]));

        let outcome = fixture.service().open_shell("fix-auth").unwrap();

        assert_eq!(
            outcome,
            ShellOutcome::Ephemeral(EphemeralShell::new(
                "/bin/example-shell",
                session.worktree_path()
            ))
        );
        assert!(fixture.multiplexer().state_changing_calls().is_empty());
    }

    #[test]
    fn a_missing_worktree_fails_before_tmux_runs() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_multiplexer(FakeMultiplexer::new().with_live(ServiceFixture::waiting(&session)));

        let error = fixture.service().open_shell("fix-auth").unwrap_err();

        assert!(
            error
                .to_string()
                .contains("is missing, run ringleader doctor")
        );
        assert!(fixture.multiplexer().state_changing_calls().is_empty());
        assert!(fixture.multiplexer().attached().is_empty());
    }

    #[test]
    fn an_unmanaged_session_gets_no_shell() {
        let stranger = ServiceFixture::session("mystery");
        let fixture = ServiceFixture::new().with_multiplexer(
            FakeMultiplexer::new().with_live(ServiceFixture::waiting(&stranger).without_metadata()),
        );

        let error = fixture.service().open_shell("mystery").unwrap_err();

        assert!(error.to_string().contains("unmanaged"));
        assert!(fixture.multiplexer().attached().is_empty());
    }

    #[test]
    fn an_unmanaged_session_is_attachable() {
        let stranger = ServiceFixture::session("mystery");
        let fixture = ServiceFixture::new().with_multiplexer(
            FakeMultiplexer::new().with_live(ServiceFixture::waiting(&stranger).without_metadata()),
        );

        assert!(fixture.service().attach("mystery").is_ok());
    }

    #[test]
    fn a_terminal_opens_in_the_worktree_of_a_stopped_session() {
        let fixture = TerminalFixture::stopped();

        fixture.service().open_terminal("fix-auth").unwrap();

        assert_eq!(
            fixture.terminal_emulator().opened(),
            [ServiceFixture::worktree_path("fix-auth")]
        );
    }

    #[test]
    fn a_terminal_opens_while_the_config_failed_to_load() {
        let fixture = TerminalFixture::stopped().with_broken_config(ConfigError::Parse {
            path: "/config/ringleader/config.toml".into(),
            detail: "expected `=`".to_string(),
        });

        fixture.service().open_terminal("fix-auth").unwrap();

        assert_eq!(fixture.terminal_emulator().opened().len(), 1);
    }

    #[test]
    fn a_terminal_for_a_live_session_sends_nothing_to_tmux_and_changes_nothing() {
        let fixture = TerminalFixture::live();

        fixture.service().open_terminal("fix-auth").unwrap();

        assert!(fixture.multiplexer().calls().is_empty());
        assert!(fixture.workspace().calls().is_empty());
        assert_eq!(fixture.registry().save_count(), 0);
        assert_eq!(fixture.terminal_emulator().opened().len(), 1);
    }

    #[test]
    fn a_missing_worktree_opens_no_terminal() {
        let fixture = ServiceFixture::new().with_sessions(&[ServiceFixture::session("fix-auth")]);

        let error = fixture.service().open_terminal("fix-auth").unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "worktree {} is missing, run ringleader doctor",
                ServiceFixture::worktree_path("fix-auth").display()
            )
        );
        assert!(fixture.terminal_emulator().opened().is_empty());
    }

    #[test]
    fn an_unmanaged_session_opens_no_terminal() {
        let stranger = ServiceFixture::session("mystery");
        let fixture = ServiceFixture::new()
            .with_host(ServiceFixture::host_for(&[&stranger]))
            .with_multiplexer(
                FakeMultiplexer::new()
                    .with_live(ServiceFixture::waiting(&stranger).without_metadata()),
            );

        let error = fixture.service().open_terminal("mystery").unwrap_err();

        assert!(matches!(error, ServiceError::NotFound(_)));
        assert!(fixture.terminal_emulator().opened().is_empty());
    }

    #[test]
    fn a_terminal_that_cannot_open_says_why() {
        let missing = TerminalEmulatorError::NoTerminal {
            reason: "none is installed".to_string(),
        };
        let fixture = TerminalFixture::stopped()
            .with_terminal_emulator(FakeTerminalEmulator::failing(missing.clone()));

        let error = fixture.service().open_terminal("fix-auth").unwrap_err();

        assert_eq!(error.to_string(), missing.to_string());
    }

    #[test]
    fn a_looked_up_pull_request_opens_at_its_url_in_the_browser() {
        let fixture = PullRequestFixture::with_pull_request();
        let service = PullRequestFixture::looked_up(&fixture);

        service.open_pull_request(PullRequestFixture::NAME).unwrap();

        assert_eq!(
            fixture.web_browser().opened(),
            [ServiceFixture::pull_request_url(PullRequestFixture::NUMBER)]
        );
    }

    #[test]
    fn a_session_not_yet_looked_up_has_its_pull_request_looked_up_and_opened() {
        let fixture = PullRequestFixture::with_pull_request();

        fixture
            .service()
            .open_pull_request(PullRequestFixture::NAME)
            .unwrap();

        assert_eq!(
            fixture.web_browser().opened(),
            [ServiceFixture::pull_request_url(PullRequestFixture::NUMBER)]
        );
    }

    #[test]
    fn a_branch_without_a_pull_request_has_no_known_pull_request_to_open() {
        let fixture = PullRequestFixture::without_pull_request();

        let error = PullRequestFixture::refusal(&fixture);

        assert!(matches!(error, ServiceError::NotFound(_)));
        assert_eq!(error.to_string(), "no pull request is known for fix-auth");
        assert!(fixture.web_browser().opened().is_empty());
    }

    #[test]
    fn a_failed_lookup_says_the_pull_request_could_not_be_looked_up() {
        let fixture = PullRequestFixture::failing_lookup();

        let error = PullRequestFixture::refusal(&fixture);

        assert!(matches!(error, ServiceError::Failed(_)));
        assert!(
            error
                .to_string()
                .starts_with("could not look up the pull request of fix-auth: ")
        );
        assert!(fixture.web_browser().opened().is_empty());
    }

    #[test]
    fn a_main_checkout_session_has_no_known_pull_request_to_open() {
        let fixture = PullRequestFixture::in_main_checkout_with_pull_request();

        let error = PullRequestFixture::refusal(&fixture);

        assert_eq!(error.to_string(), "no pull request is known for fix-auth");
        assert!(fixture.web_browser().opened().is_empty());
    }

    #[test]
    fn a_session_with_no_entry_has_no_known_pull_request_to_open() {
        let fixture = PullRequestFixture::with_pull_request();
        let service = PullRequestFixture::looked_up(&fixture);

        let error = service.open_pull_request("mystery").unwrap_err();

        assert_eq!(error.to_string(), "no pull request is known for mystery");
        assert!(fixture.web_browser().opened().is_empty());
    }

    #[test]
    fn a_pull_request_opens_while_the_config_failed_to_load() {
        let fixture =
            PullRequestFixture::with_pull_request().with_broken_config(ConfigError::Parse {
                path: "/config/ringleader/config.toml".into(),
                detail: "expected `=`".to_string(),
            });
        let service = PullRequestFixture::looked_up(&fixture);

        service.open_pull_request(PullRequestFixture::NAME).unwrap();

        assert_eq!(fixture.web_browser().opened().len(), 1);
    }

    #[test]
    fn opening_a_pull_request_sends_nothing_to_tmux_or_git_and_changes_nothing() {
        let fixture = PullRequestFixture::with_pull_request();
        let service = PullRequestFixture::looked_up(&fixture);

        service.open_pull_request(PullRequestFixture::NAME).unwrap();

        assert!(fixture.multiplexer().calls().is_empty());
        assert!(fixture.workspace().calls().is_empty());
        assert_eq!(fixture.registry().save_count(), 0);
    }

    #[test]
    fn a_browser_that_cannot_open_says_why() {
        let missing = WebBrowserError::NoBrowser;
        let fixture = PullRequestFixture::with_pull_request()
            .with_web_browser(FakeWebBrowser::failing(missing.clone()));
        let service = PullRequestFixture::looked_up(&fixture);

        let error = service
            .open_pull_request(PullRequestFixture::NAME)
            .unwrap_err();

        assert!(matches!(error, ServiceError::Failed(_)));
        assert_eq!(error.to_string(), missing.to_string());
    }
}
