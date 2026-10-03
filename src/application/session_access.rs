use std::sync::Arc;

use super::ports::{AttachTarget, Multiplexer, ReturnBinding};
use super::service_error::ServiceError;
use super::service_ports::TmuxPorts;
use crate::domain::{SessionState, SessionSummary};

pub struct SessionAccess {
    multiplexer: Arc<dyn Multiplexer>,
    binding: Arc<dyn ReturnBinding>,
    shell: String,
}

impl SessionAccess {
    pub fn new(tmux: &TmuxPorts, shell: &str) -> Self {
        Self {
            multiplexer: tmux.multiplexer(),
            binding: tmux.binding(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::BindingError;
    use crate::test_support::{FakeMultiplexer, FakeReturnBinding, ServiceFixture};

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
