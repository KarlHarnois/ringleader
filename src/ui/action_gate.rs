use super::app::{AppRequest, BlockingOperation};
use crate::application::ports::ConfigError;
use crate::domain::{SessionState, SessionSummary};

pub struct ActionGate<'a> {
    selected: Option<&'a SessionSummary>,
    config_error: Option<&'a ConfigError>,
}

impl<'a> ActionGate<'a> {
    const NO_SELECTION: &'static str = "No session is selected.";

    pub fn new(
        selected: Option<&'a SessionSummary>,
        config_error: Option<&'a ConfigError>,
    ) -> Self {
        Self {
            selected,
            config_error,
        }
    }

    pub fn attach(&self) -> Result<AppRequest, String> {
        Ok(AppRequest::Attach(self.selected()?.name().to_string()))
    }

    pub fn shell(&self) -> Result<AppRequest, String> {
        let row = self.selected()?;
        let name = row.name();
        match row.state() {
            SessionState::Unmanaged => Err(format!(
                "{name} is unmanaged, so t cannot open a shell in it."
            )),
            _ => Ok(AppRequest::Shell(name.to_string())),
        }
    }

    pub fn restart(&self) -> Result<AppRequest, String> {
        self.writable()?;
        let row = self.selected()?;
        let name = row.name();
        match row.state() {
            SessionState::Exited | SessionState::Stopped => Ok(AppRequest::Block(
                BlockingOperation::Restart(name.to_string()),
            )),
            SessionState::Unmanaged => {
                Err(format!("{name} is unmanaged, so it cannot be restarted."))
            }
            state => Err(format!(
                "{name} is {}, so it cannot be restarted.",
                state.label()
            )),
        }
    }

    fn writable(&self) -> Result<(), String> {
        match self.config_error {
            Some(error) => Err(format!("config error, running read-only: {error}")),
            None => Ok(()),
        }
    }

    fn selected(&self) -> Result<&'a SessionSummary, String> {
        self.selected.ok_or_else(|| Self::NO_SELECTION.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::domain::UnmanagedSession;
    use crate::test_support::ServiceFixture;

    struct GateFixture;

    impl GateFixture {
        fn row(state: SessionState) -> SessionSummary {
            SessionSummary::managed(ServiceFixture::session("fix-auth"), state, false)
        }

        fn config_error() -> ConfigError {
            ConfigError::Parse {
                path: PathBuf::from("/config/ringleader/config.toml"),
                detail: "expected `=`".to_string(),
            }
        }
    }

    #[test]
    fn exited_and_stopped_sessions_restart() {
        for state in [SessionState::Exited, SessionState::Stopped] {
            let row = GateFixture::row(state);

            assert_eq!(
                ActionGate::new(Some(&row), None).restart(),
                Ok(AppRequest::Block(BlockingOperation::Restart(
                    "fix-auth".to_string()
                )))
            );
        }
    }

    #[test]
    fn a_working_session_cannot_restart() {
        let row = GateFixture::row(SessionState::Working);

        assert_eq!(
            ActionGate::new(Some(&row), None).restart(),
            Err("fix-auth is working, so it cannot be restarted.".to_string())
        );
    }

    #[test]
    fn a_waiting_session_cannot_restart() {
        let row = GateFixture::row(SessionState::Waiting);

        assert!(ActionGate::new(Some(&row), None).restart().is_err());
    }

    #[test]
    fn an_unmanaged_session_cannot_restart() {
        let stranger = ServiceFixture::session("mystery");
        let row = SessionSummary::unmanaged(UnmanagedSession::new(
            "mystery",
            ServiceFixture::exited(&stranger).without_metadata(),
        ));

        assert_eq!(
            ActionGate::new(Some(&row), None).restart(),
            Err("mystery is unmanaged, so it cannot be restarted.".to_string())
        );
    }

    #[test]
    fn a_read_only_tui_refuses_a_restart_with_the_config_error() {
        let row = GateFixture::row(SessionState::Exited);
        let error = GateFixture::config_error();

        let refusal = ActionGate::new(Some(&row), Some(&error))
            .restart()
            .unwrap_err();

        assert!(refusal.starts_with("config error, running read-only: "));
        assert!(refusal.contains("expected `=`"));
    }

    #[test]
    fn a_read_only_tui_still_attaches() {
        let row = GateFixture::row(SessionState::Waiting);
        let error = GateFixture::config_error();

        assert!(ActionGate::new(Some(&row), Some(&error)).attach().is_ok());
    }

    #[test]
    fn a_managed_session_opens_a_shell_even_when_read_only() {
        let row = GateFixture::row(SessionState::Stopped);
        let error = GateFixture::config_error();

        assert_eq!(
            ActionGate::new(Some(&row), Some(&error)).shell(),
            Ok(AppRequest::Shell("fix-auth".to_string()))
        );
    }

    #[test]
    fn an_unmanaged_session_opens_no_shell() {
        let stranger = ServiceFixture::session("mystery");
        let row = SessionSummary::unmanaged(UnmanagedSession::new(
            "mystery",
            ServiceFixture::waiting(&stranger).without_metadata(),
        ));

        assert_eq!(
            ActionGate::new(Some(&row), None).shell(),
            Err("mystery is unmanaged, so t cannot open a shell in it.".to_string())
        );
    }

    #[test]
    fn nothing_selected_refuses_every_action() {
        let gate = ActionGate::new(None, None);
        let refusal = Err("No session is selected.".to_string());

        assert_eq!(gate.attach(), refusal);
        assert_eq!(gate.shell(), refusal);
        assert_eq!(gate.restart(), refusal);
    }
}
