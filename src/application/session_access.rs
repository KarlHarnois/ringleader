use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::ports::{AttachTarget, HostProbe, Multiplexer, ReturnBinding};
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use crate::domain::{Session, SessionState, SessionSummary};

pub struct SessionAccess {
    multiplexer: Arc<dyn Multiplexer>,
    binding: Arc<dyn ReturnBinding>,
    host: Arc<dyn HostProbe>,
    shell: String,
}

impl SessionAccess {
    pub fn new(ports: &ServicePorts, shell: &str) -> Self {
        Self {
            multiplexer: ports.tmux().multiplexer(),
            binding: ports.tmux().binding(),
            host: ports.repos().host(),
            shell: shell.to_string(),
        }
    }

    pub fn attach(
        &self,
        row: &SessionSummary,
        mode: ReturnMode,
    ) -> Result<AttachOutcome, ServiceError> {
        if row.state() == SessionState::Stopped {
            return Err(ServiceError::Conflict(format!(
                "session {} is stopped, restart it first",
                row.name()
            )));
        }
        let outcome = self.prepare_return(mode)?;
        self.multiplexer
            .attach(&AttachTarget::Agent(row.tmux_name().to_string()))?;
        Ok(outcome)
    }

    pub fn open_shell(
        &self,
        row: &SessionSummary,
        mode: ReturnMode,
    ) -> Result<ShellOutcome, ServiceError> {
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
        let outcome = self.prepare_return(mode)?;
        let window = self.multiplexer.ensure_shell_window(
            session.tmux_name(),
            session.worktree_path(),
            &self.shell,
        )?;
        self.multiplexer.attach(&AttachTarget::Shell(window))?;
        Ok(ShellOutcome::Attached(outcome))
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

    fn prepare_return(&self, mode: ReturnMode) -> Result<AttachOutcome, ServiceError> {
        if mode == ReturnMode::None {
            return Ok(AttachOutcome::default());
        }
        match self.binding.install() {
            Ok(()) => Ok(AttachOutcome::default()),
            Err(error) if error.is_timeout() => {
                Err(ServiceError::Failed(format!("{error}, attach cancelled")))
            }
            Err(error) => Ok(AttachOutcome {
                binding_warning: Some(error.to_string()),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnMode {
    InstallReturnBinding,
    None,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttachOutcome {
    binding_warning: Option<String>,
}

impl AttachOutcome {
    pub fn binding_warning(&self) -> Option<&str> {
        self.binding_warning.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellOutcome {
    Attached(AttachOutcome),
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
    use crate::application::ports::BindingError;
    use crate::test_support::{FakeMultiplexer, FakeReturnBinding, ServiceFixture};

    struct AttachFixture;

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

    #[test]
    fn attach_targets_the_agent_pane_of_the_session() {
        let fixture = AttachFixture::live();

        fixture
            .service()
            .attach("fix-auth", ReturnMode::None)
            .unwrap();

        assert_eq!(
            fixture.multiplexer().attached(),
            [AttachTarget::Agent("ringleader-fix-auth".to_string())]
        );
    }

    #[test]
    fn attach_from_the_cli_never_touches_the_return_binding() {
        let fixture = AttachFixture::live();

        fixture
            .service()
            .attach("fix-auth", ReturnMode::None)
            .unwrap();

        assert!(fixture.binding().calls().is_empty());
    }

    #[test]
    fn attach_from_the_tui_installs_the_return_binding_first() {
        let fixture = AttachFixture::live();

        fixture
            .service()
            .attach("fix-auth", ReturnMode::InstallReturnBinding)
            .unwrap();

        assert_eq!(fixture.binding().calls(), ["install"]);
    }

    #[test]
    fn a_timed_out_install_cancels_the_attach() {
        let fixture = AttachFixture::live().with_binding(
            FakeReturnBinding::default().failing_install(BindingError::TimedOut {
                subcommand: "bind-key".to_string(),
                seconds: 10,
            }),
        );

        let error = fixture
            .service()
            .attach("fix-auth", ReturnMode::InstallReturnBinding)
            .unwrap_err();

        assert!(error.to_string().contains("attach cancelled"));
        assert!(fixture.multiplexer().attached().is_empty());
    }

    #[test]
    fn a_failed_install_still_attaches_and_reports_the_warning() {
        let fixture = AttachFixture::live().with_binding(
            FakeReturnBinding::default().failing_install(BindingError::Unavailable {
                reason: "TMUX_PANE is not set".to_string(),
            }),
        );

        let outcome = fixture
            .service()
            .attach("fix-auth", ReturnMode::InstallReturnBinding)
            .unwrap();

        assert!(outcome.binding_warning().unwrap().contains("TMUX_PANE"));
        assert_eq!(fixture.multiplexer().attached().len(), 1);
    }

    #[test]
    fn attach_refuses_a_stopped_session() {
        let fixture = ServiceFixture::new().with_sessions(&[ServiceFixture::session("fix-auth")]);

        let error = fixture
            .service()
            .attach("fix-auth", ReturnMode::None)
            .unwrap_err();

        assert!(error.to_string().contains("stopped"));
    }

    #[test]
    fn a_shell_in_a_live_session_attaches_to_its_shell_window() {
        let fixture = ShellFixture::live();

        let outcome = fixture
            .service()
            .open_shell("fix-auth", ReturnMode::None)
            .unwrap();

        assert_eq!(outcome, ShellOutcome::Attached(AttachOutcome::default()));
        assert_eq!(
            fixture.multiplexer().attached(),
            [AttachTarget::Shell("@9".to_string())]
        );
    }

    #[test]
    fn a_shell_in_a_live_session_runs_the_configured_shell() {
        let fixture = ShellFixture::live();

        fixture
            .service()
            .open_shell("fix-auth", ReturnMode::None)
            .unwrap();

        assert!(
            fixture.multiplexer().calls().contains(
                &"ensure_shell_window ringleader-fix-auth /bin/example-shell".to_string()
            )
        );
    }

    #[test]
    fn a_shell_from_the_tui_installs_the_return_binding_first() {
        let fixture = ShellFixture::live();

        fixture
            .service()
            .open_shell("fix-auth", ReturnMode::InstallReturnBinding)
            .unwrap();

        assert_eq!(fixture.binding().calls(), ["install"]);
    }

    #[test]
    fn a_shell_in_a_stopped_session_is_an_ephemeral_shell_in_its_worktree() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_host(ServiceFixture::host_for(&[&session]));

        let outcome = fixture
            .service()
            .open_shell("fix-auth", ReturnMode::None)
            .unwrap();

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

        let error = fixture
            .service()
            .open_shell("fix-auth", ReturnMode::None)
            .unwrap_err();

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

        let error = fixture
            .service()
            .open_shell("mystery", ReturnMode::None)
            .unwrap_err();

        assert!(error.to_string().contains("unmanaged"));
        assert!(fixture.multiplexer().attached().is_empty());
    }

    #[test]
    fn an_unmanaged_session_is_attachable() {
        let stranger = ServiceFixture::session("mystery");
        let fixture = ServiceFixture::new().with_multiplexer(
            FakeMultiplexer::new().with_live(ServiceFixture::waiting(&stranger).without_metadata()),
        );

        assert!(
            fixture
                .service()
                .attach("mystery", ReturnMode::None)
                .is_ok()
        );
    }
}
