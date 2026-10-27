use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::checks::agent_readiness::AgentReadiness;
use super::ports::{
    AttachTarget, ClientView, HostProbe, Multiplexer, MultiplexerError, RepoPages, ReturnRequest,
    TerminalEmulator, WebBrowser,
};
use super::reconciliation::ManagedRow;
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use crate::domain::{PullRequest, RepoGroup, Session, SessionState, SessionSummary};

pub struct SessionAccess {
    multiplexer: Arc<dyn Multiplexer>,
    host: Arc<dyn HostProbe>,
    terminal_emulator: Arc<dyn TerminalEmulator>,
    web_browser: Arc<dyn WebBrowser>,
    repo_pages: Arc<dyn RepoPages>,
    shell: String,
}

impl SessionAccess {
    const SENDING: &'static str = "sending";

    pub fn new(ports: &ServicePorts, shell: &str) -> Self {
        Self {
            multiplexer: ports.tmux().multiplexer(),
            host: ports.repos().host(),
            terminal_emulator: ports.desktop().terminal_emulator(),
            web_browser: ports.desktop().web_browser(),
            repo_pages: ports.repos().repo_pages(),
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
        Self::refuse_stopped(row)?;
        self.refuse_own_pane()?;
        self.multiplexer.attach(&Self::agent_of(row))?;
        Ok(())
    }

    pub fn switch_client(&self, row: &SessionSummary, client: &str) -> Result<(), ServiceError> {
        Self::refuse_stopped(row)?;
        self.multiplexer
            .switch_client(client, &Self::agent_of(row))?;
        Ok(())
    }

    pub fn send_prompt(&self, row: &ManagedRow, prompt: &str) -> Result<(), ServiceError> {
        Self::refuse_blank(prompt)?;
        Self::refuse_unless_agent_runs(row)?;
        let session = row.session();
        match self
            .multiplexer
            .submit_when_ready(session.tmux_name(), prompt)
        {
            Err(MultiplexerError::AgentNotReady { .. }) => {
                Err(AgentReadiness::not_at_prompt(session.name(), Self::SENDING))
            }
            submitted => Ok(submitted?),
        }
    }

    fn refuse_stopped(row: &SessionSummary) -> Result<(), ServiceError> {
        if row.state() != SessionState::Stopped {
            return Ok(());
        }
        Err(ServiceError::Conflict(format!(
            "session {} is stopped, restart it first",
            row.name()
        )))
    }

    fn agent_of(row: &SessionSummary) -> AttachTarget {
        AttachTarget::Agent(row.tmux_name().to_string())
    }

    pub fn open_shell(&self, row: &SessionSummary) -> Result<ShellOutcome, ServiceError> {
        let session = Self::trusted_session(row)?;
        self.check_worktree(session)?;
        if row.state() == SessionState::Stopped {
            let shell = EphemeralShell::new(&self.shell, session.worktree_path());
            return Ok(ShellOutcome::Ephemeral(shell));
        }
        self.refuse_own_pane()?;
        self.multiplexer.attach(&self.shell_window(session)?)?;
        Ok(ShellOutcome::Attached)
    }

    pub fn client_view(&self, client: &str) -> Result<ClientView, ServiceError> {
        self.multiplexer
            .client_view(client)?
            .ok_or_else(|| ServiceError::NotFound(format!("tmux client {client} shows no session")))
    }

    pub fn toggle_shell(
        &self,
        row: &SessionSummary,
        view: &ClientView,
        client: &str,
    ) -> Result<(), ServiceError> {
        let target = match view.shows_shell() {
            true => Self::agent_of(row),
            false => self.checked_shell_window(row)?,
        };
        self.multiplexer.switch_client(client, &target)?;
        Ok(())
    }

    pub fn return_client(&self, client: &str) -> Result<(), ServiceError> {
        self.multiplexer.detach_client(client)?;
        Ok(())
    }

    pub fn delete_on_return(&self, row: &SessionSummary, client: &str) -> Result<(), ServiceError> {
        let request = ReturnRequest::Delete(row.tmux_name().to_string());
        self.multiplexer.record_return_request(&request)?;
        let detached = self.multiplexer.detach_client(client);
        if detached.is_err() {
            self.withdraw_return_request();
        }
        Ok(detached?)
    }

    pub fn return_request(&self) -> Result<Option<ReturnRequest>, ServiceError> {
        Ok(self.multiplexer.return_request()?)
    }

    pub fn clear_return_request(&self) -> Result<(), ServiceError> {
        self.multiplexer.clear_return_request()?;
        Ok(())
    }

    fn withdraw_return_request(&self) {
        if let Err(error) = self.multiplexer.clear_return_request() {
            log::warn!("could not withdraw the return request: {error}");
        }
    }

    fn trusted_session(row: &SessionSummary) -> Result<&Session, ServiceError> {
        row.session().ok_or_else(|| {
            ServiceError::Invalid(format!(
                "session {} is unmanaged and has no trusted worktree for a shell",
                row.name()
            ))
        })
    }

    fn checked_shell_window(&self, row: &SessionSummary) -> Result<AttachTarget, ServiceError> {
        let session = Self::trusted_session(row)?;
        self.check_worktree(session)?;
        self.shell_window(session)
    }

    fn shell_window(&self, session: &Session) -> Result<AttachTarget, ServiceError> {
        let window = self.multiplexer.ensure_shell_window(
            session.tmux_name(),
            session.worktree_path(),
            &self.shell,
        )?;
        Ok(AttachTarget::Shell(window))
    }

    pub fn open_terminal(&self, session: &Session) -> Result<(), ServiceError> {
        self.check_worktree(session)?;
        self.terminal_emulator
            .open_window(session.worktree_path())?;
        Ok(())
    }

    pub fn open_repo_terminal(&self, group: &RepoGroup) -> Result<(), ServiceError> {
        let main_checkout = self.main_checkout_of(group, "main checkout")?;
        self.terminal_emulator.open_window(main_checkout)?;
        Ok(())
    }

    pub fn open_repo_page(&self, group: &RepoGroup) -> Result<(), ServiceError> {
        let main_checkout = self.main_checkout_of(group, "GitHub page")?;
        let page = self.repo_pages.main_page(main_checkout)?.ok_or_else(|| {
            ServiceError::NotFound(format!("no GitHub page is known for {group}"))
        })?;
        self.web_browser.open(&page)?;
        Ok(())
    }

    pub fn open_pull_request(&self, pull_request: &PullRequest) -> Result<(), ServiceError> {
        self.web_browser.open(pull_request.url())?;
        Ok(())
    }

    fn refuse_blank(prompt: &str) -> Result<(), ServiceError> {
        if prompt.trim().is_empty() {
            return Err(ServiceError::Invalid(
                "the prompt is empty, pass the text the agent should read".to_string(),
            ));
        }
        Ok(())
    }

    fn refuse_unless_agent_runs(row: &ManagedRow) -> Result<(), ServiceError> {
        if row.state().runs_agent() {
            return Ok(());
        }
        Err(ServiceError::Conflict(format!(
            "session {} is {}, restart it before sending it a prompt",
            row.session().name(),
            row.state().label()
        )))
    }

    fn check_worktree(&self, session: &Session) -> Result<(), ServiceError> {
        let worktree = session.worktree_path();
        let missing = format!(
            "worktree {} is missing, run ringleader doctor",
            worktree.display()
        );
        self.check_present(worktree, missing)
    }

    fn main_checkout_of<'g>(
        &self,
        group: &'g RepoGroup,
        target: &str,
    ) -> Result<&'g Path, ServiceError> {
        let main_checkout = group.path().ok_or_else(|| {
            ServiceError::Invalid(format!(
                "the sessions under {group} belong to no repo, so there is no {target} to open"
            ))
        })?;
        self.check_main_checkout(main_checkout)?;
        Ok(main_checkout)
    }

    fn check_main_checkout(&self, main_checkout: &Path) -> Result<(), ServiceError> {
        let missing = format!("main checkout {} is missing", main_checkout.display());
        self.check_present(main_checkout, missing)
    }

    fn check_present(&self, directory: &Path, missing: String) -> Result<(), ServiceError> {
        if self.host.path_exists(directory) {
            return Ok(());
        }
        Err(ServiceError::Invalid(missing))
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
    use crate::application::ports::{
        ConfigError, RepoPageError, TerminalEmulatorError, WebBrowserError,
    };
    use crate::domain::PullRequestState;
    use crate::test_support::{
        FakeMultiplexer, FakePullRequests, FakeRepoPages, FakeTerminalEmulator, FakeWebBrowser,
        ServiceFixture,
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

    struct ClientFixture;

    impl ClientFixture {
        const CLIENT: &'static str = "client-1234";
        const TMUX_NAME: &'static str = "ringleader-fix-auth";

        fn on_agent() -> ServiceFixture {
            Self::showing(
                ClientView::showing_agent(Self::TMUX_NAME),
                FakeMultiplexer::new(),
            )
        }

        fn on_shell() -> ServiceFixture {
            Self::showing(
                ClientView::showing_shell(Self::TMUX_NAME),
                FakeMultiplexer::new(),
            )
        }

        fn with_delete_request() -> ServiceFixture {
            let multiplexer = FakeMultiplexer::new().with_return_request(Self::delete_request());
            Self::showing(ClientView::showing_agent(Self::TMUX_NAME), multiplexer)
        }

        fn failing_to_detach() -> ServiceFixture {
            let failure = MultiplexerError::Failed {
                subcommand: "detach-client".to_string(),
                detail: "example failure".to_string(),
            };
            let multiplexer = FakeMultiplexer::new().failing("detach_client", failure);
            Self::showing(ClientView::showing_agent(Self::TMUX_NAME), multiplexer)
        }

        fn showing(view: ClientView, multiplexer: FakeMultiplexer) -> ServiceFixture {
            let session = ServiceFixture::session("fix-auth");
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&session))
                .with_host(ServiceFixture::host_for(&[&session]))
                .with_multiplexer(
                    multiplexer
                        .with_live(ServiceFixture::waiting(&session))
                        .with_client_view(Self::CLIENT, view),
                )
        }

        fn delete_request() -> ReturnRequest {
            ReturnRequest::Delete(Self::TMUX_NAME.to_string())
        }

        fn switched_to(target: AttachTarget) -> Vec<(String, AttachTarget)> {
            vec![(Self::CLIENT.to_string(), target)]
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

        fn open_error(fixture: &ServiceFixture) -> ServiceError {
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

        fn repo() -> RepoGroup {
            ServiceFixture::repo_group(ServiceFixture::REPO)
        }
    }

    struct RepoPageFixture;

    impl RepoPageFixture {
        const VANISHED: &'static str = "/repos/vanished";

        fn on_github() -> ServiceFixture {
            ServiceFixture::new().with_repo_pages(Self::pages())
        }

        fn pages() -> FakeRepoPages {
            FakeRepoPages::default()
                .with_page(Path::new(ServiceFixture::REPO), ServiceFixture::REPO_PAGE)
                .with_page(Path::new(Self::VANISHED), ServiceFixture::REPO_PAGE)
        }

        fn repo() -> RepoGroup {
            ServiceFixture::repo_group(ServiceFixture::REPO)
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
    fn switching_a_client_targets_the_agent_pane_of_the_session() {
        let fixture = AttachFixture::live();

        fixture
            .service()
            .switch_client("fix-auth", "client-1234")
            .unwrap();

        assert_eq!(
            fixture.multiplexer().switched(),
            [(
                "client-1234".to_string(),
                AttachTarget::Agent("ringleader-fix-auth".to_string())
            )]
        );
    }

    #[test]
    fn switching_a_client_from_a_pane_of_the_own_server_is_allowed() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_multiplexer(
                FakeMultiplexer::new()
                    .with_live(ServiceFixture::waiting(&session))
                    .called_from_own_pane(),
            );

        fixture
            .service()
            .switch_client("fix-auth", "client-1234")
            .unwrap();

        assert_eq!(fixture.multiplexer().switched().len(), 1);
    }

    #[test]
    fn switching_a_client_refuses_a_stopped_session_as_attach_does() {
        let fixture = ServiceFixture::new().with_sessions(&[ServiceFixture::session("fix-auth")]);

        let switching = fixture
            .service()
            .switch_client("fix-auth", "client-1234")
            .unwrap_err();
        let attaching = fixture.service().attach("fix-auth").unwrap_err();

        assert_eq!(switching.to_string(), attaching.to_string());
        assert!(fixture.multiplexer().switched().is_empty());
    }

    #[test]
    fn a_client_names_the_session_it_shows() {
        let fixture = ClientFixture::on_agent();

        let name = fixture
            .service()
            .client_session(ClientFixture::CLIENT)
            .unwrap();

        assert_eq!(name, "fix-auth");
    }

    #[test]
    fn a_client_that_shows_no_session_is_not_found() {
        let fixture = ClientFixture::on_agent();

        let error = fixture.service().client_session("client-9999").unwrap_err();

        assert!(matches!(error, ServiceError::NotFound(_)), "{error}");
    }

    #[test]
    fn toggling_from_the_agent_shows_the_shell_window_of_the_configured_shell() {
        let fixture = ClientFixture::on_agent();

        fixture
            .service()
            .toggle_shell(ClientFixture::CLIENT)
            .unwrap();

        assert_eq!(
            fixture.multiplexer().switched(),
            ClientFixture::switched_to(AttachTarget::Shell("@9".to_string()))
        );
        assert!(
            fixture.multiplexer().calls().contains(
                &"ensure_shell_window ringleader-fix-auth /bin/example-shell".to_string()
            )
        );
    }

    #[test]
    fn toggling_from_the_shell_shows_the_agent() {
        let fixture = ClientFixture::on_shell();

        fixture
            .service()
            .toggle_shell(ClientFixture::CLIENT)
            .unwrap();

        assert_eq!(
            fixture.multiplexer().switched(),
            ClientFixture::switched_to(AttachTarget::Agent(ClientFixture::TMUX_NAME.to_string()))
        );
    }

    #[test]
    fn returning_a_client_detaches_it() {
        let fixture = ClientFixture::on_agent();

        fixture
            .service()
            .return_client(ClientFixture::CLIENT)
            .unwrap();

        assert_eq!(fixture.multiplexer().detached(), [ClientFixture::CLIENT]);
    }

    #[test]
    fn a_delete_on_return_records_the_request_before_it_detaches_the_client() {
        let fixture = ClientFixture::on_agent();

        fixture
            .service()
            .delete_on_return(ClientFixture::CLIENT)
            .unwrap();

        let calls = fixture.multiplexer().state_changing_calls();
        assert_eq!(
            calls,
            [
                format!(
                    "record_return_request {:?}",
                    ClientFixture::delete_request()
                ),
                format!("detach_client {}", ClientFixture::CLIENT),
            ]
        );
    }

    #[test]
    fn a_delete_on_return_whose_detach_fails_withdraws_the_request() {
        let fixture = ClientFixture::failing_to_detach();

        let outcome = fixture.service().delete_on_return(ClientFixture::CLIENT);

        assert!(outcome.is_err());
        assert_eq!(fixture.multiplexer().recorded_return_request(), None);
    }

    #[test]
    fn a_recorded_delete_request_names_the_session_to_delete() {
        let fixture = ClientFixture::with_delete_request();

        let requested = fixture.service().requested_deletion().unwrap();

        assert_eq!(requested.as_deref(), Some("fix-auth"));
    }

    #[test]
    fn no_recorded_request_names_no_session_to_delete() {
        let fixture = ClientFixture::on_agent();

        assert_eq!(fixture.service().requested_deletion().unwrap(), None);
    }

    #[test]
    fn a_cleared_request_names_no_session_to_delete() {
        let fixture = ClientFixture::with_delete_request();

        fixture.service().clear_return_request().unwrap();

        assert_eq!(fixture.service().requested_deletion().unwrap(), None);
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
    fn a_repo_terminal_opens_in_the_main_checkout() {
        let fixture = ServiceFixture::new();

        fixture
            .service()
            .open_repo_terminal(&TerminalFixture::repo())
            .unwrap();

        assert_eq!(
            fixture.terminal_emulator().opened(),
            [PathBuf::from(ServiceFixture::REPO)]
        );
    }

    #[test]
    fn a_repo_terminal_sends_nothing_to_tmux_and_changes_nothing() {
        let fixture = TerminalFixture::live();

        fixture
            .service()
            .open_repo_terminal(&TerminalFixture::repo())
            .unwrap();

        assert!(fixture.multiplexer().calls().is_empty());
        assert!(fixture.workspace().calls().is_empty());
        assert_eq!(fixture.registry().save_count(), 0);
    }

    #[test]
    fn a_missing_main_checkout_opens_no_terminal() {
        let fixture = ServiceFixture::new();
        let vanished = ServiceFixture::repo_group("/repos/vanished");

        let error = fixture.service().open_repo_terminal(&vanished).unwrap_err();

        assert_eq!(
            error.to_string(),
            "main checkout /repos/vanished is missing"
        );
        assert!(fixture.terminal_emulator().opened().is_empty());
    }

    #[test]
    fn sessions_outside_any_repo_have_no_main_checkout_to_open() {
        let fixture = ServiceFixture::new();

        let error = fixture
            .service()
            .open_repo_terminal(&RepoGroup::Other)
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "the sessions under other belong to no repo, so there is no main checkout to open"
        );
        assert!(fixture.terminal_emulator().opened().is_empty());
    }

    #[test]
    fn a_repo_page_opens_in_the_browser() {
        let fixture = RepoPageFixture::on_github();

        fixture
            .service()
            .open_repo_page(&RepoPageFixture::repo())
            .unwrap();

        assert_eq!(fixture.web_browser().opened(), [ServiceFixture::REPO_PAGE]);
    }

    #[test]
    fn a_repo_without_a_known_github_page_opens_nothing() {
        let fixture = ServiceFixture::new();

        let error = fixture
            .service()
            .open_repo_page(&RepoPageFixture::repo())
            .unwrap_err();

        assert!(matches!(error, ServiceError::NotFound(_)));
        assert_eq!(
            error.to_string(),
            "no GitHub page is known for example-project"
        );
        assert!(fixture.web_browser().opened().is_empty());
    }

    #[test]
    fn sessions_outside_any_repo_have_no_github_page_to_open() {
        let fixture = RepoPageFixture::on_github();

        let error = fixture
            .service()
            .open_repo_page(&RepoGroup::Other)
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "the sessions under other belong to no repo, so there is no GitHub page to open"
        );
        assert!(fixture.web_browser().opened().is_empty());
    }

    #[test]
    fn a_missing_main_checkout_opens_no_github_page() {
        let fixture = RepoPageFixture::on_github();
        let vanished = ServiceFixture::repo_group(RepoPageFixture::VANISHED);

        let error = fixture.service().open_repo_page(&vanished).unwrap_err();

        assert_eq!(
            error.to_string(),
            "main checkout /repos/vanished is missing"
        );
        assert!(fixture.web_browser().opened().is_empty());
    }

    #[test]
    fn a_failed_github_page_lookup_says_why() {
        let timeout = RepoPageError::TimedOut { seconds: 20 };
        let fixture =
            ServiceFixture::new().with_repo_pages(FakeRepoPages::failing(timeout.clone()));

        let error = fixture
            .service()
            .open_repo_page(&RepoPageFixture::repo())
            .unwrap_err();

        assert!(matches!(error, ServiceError::Failed(_)));
        assert_eq!(error.to_string(), timeout.to_string());
    }

    #[test]
    fn opening_a_repo_page_sends_nothing_to_tmux_or_git_and_changes_nothing() {
        let fixture = TerminalFixture::live().with_repo_pages(RepoPageFixture::pages());

        fixture
            .service()
            .open_repo_page(&RepoPageFixture::repo())
            .unwrap();

        assert!(fixture.multiplexer().calls().is_empty());
        assert!(fixture.workspace().calls().is_empty());
        assert_eq!(fixture.registry().save_count(), 0);
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

        let error = PullRequestFixture::open_error(&fixture);

        assert!(matches!(error, ServiceError::NotFound(_)));
        assert_eq!(error.to_string(), "no pull request is known for fix-auth");
        assert!(fixture.web_browser().opened().is_empty());
    }

    #[test]
    fn a_failed_lookup_says_the_pull_request_could_not_be_looked_up() {
        let fixture = PullRequestFixture::failing_lookup();

        let error = PullRequestFixture::open_error(&fixture);

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

        let error = PullRequestFixture::open_error(&fixture);

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
