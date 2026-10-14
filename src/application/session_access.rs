use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::ports::{AttachTarget, HostProbe, Multiplexer};
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use crate::domain::{Session, SessionState, SessionSummary};

pub struct SessionAccess {
    multiplexer: Arc<dyn Multiplexer>,
    host: Arc<dyn HostProbe>,
    shell: String,
}

impl SessionAccess {
    pub fn new(ports: &ServicePorts, shell: &str) -> Self {
        Self {
            multiplexer: ports.tmux().multiplexer(),
            host: ports.repos().host(),
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
    use crate::test_support::{FakeMultiplexer, ServiceFixture};

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
}
