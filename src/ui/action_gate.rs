use crossterm::event::{KeyCode, KeyEvent};

use super::app::AppRequest;
use super::blocking_operation::BlockingOperation;
use super::dialogs::DialogOpening;
use crate::application::ports::ConfigError;
use crate::domain::{Session, SessionState, SessionSummary};

pub struct ActionGate<'a> {
    selected: Option<&'a SessionSummary>,
    config_error: Option<&'a ConfigError>,
    heading_selected: bool,
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
            heading_selected: false,
        }
    }

    pub fn on_heading(config_error: Option<&'a ConfigError>) -> Self {
        Self {
            selected: None,
            config_error,
            heading_selected: true,
        }
    }

    pub fn action_for(&self, key: &KeyEvent) -> KeyAction {
        match key.code {
            KeyCode::Char('q') => KeyAction::Request(AppRequest::Quit),
            KeyCode::Char('r') => KeyAction::Reload,
            KeyCode::Char('?') => KeyAction::Open(DialogOpening::Help),
            KeyCode::Down | KeyCode::Right | KeyCode::Char('j' | 'l') => KeyAction::Move(1),
            KeyCode::Up | KeyCode::Left | KeyCode::Char('k' | 'h') => KeyAction::Move(-1),
            KeyCode::Char('G') => KeyAction::SelectLast,
            KeyCode::Char('n') => KeyAction::from(
                self.writable()
                    .map(|()| KeyAction::Open(DialogOpening::NewSession)),
            ),
            KeyCode::Char(' ') => KeyAction::Fold,
            KeyCode::Char(digit @ '1'..='9') => KeyAction::JumpToRepo(Self::repo_number(digit)),
            KeyCode::Enter => self.enter(),
            KeyCode::Char('t') => KeyAction::from(self.shell().map(KeyAction::Request)),
            KeyCode::Char('s') => KeyAction::from(self.restart().map(KeyAction::Request)),
            KeyCode::Char('d') => KeyAction::from(self.delete().map(KeyAction::Request)),
            KeyCode::Char('m') => KeyAction::from(self.merge().map(KeyAction::Request)),
            KeyCode::Char('R') => KeyAction::from(
                self.rename()
                    .map(|session| KeyAction::Open(DialogOpening::Rename(Box::new(session)))),
            ),
            _ => KeyAction::Ignore,
        }
    }

    fn repo_number(digit: char) -> usize {
        let number = digit.to_digit(10).unwrap_or_default();
        usize::try_from(number).unwrap_or_default()
    }

    fn enter(&self) -> KeyAction {
        if self.heading_selected {
            return KeyAction::Fold;
        }
        KeyAction::from(self.attach().map(KeyAction::Request))
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

    pub fn delete(&self) -> Result<AppRequest, String> {
        self.writable()?;
        let row = self.selected()?;
        let name = row.name();
        if row.state() == SessionState::Unmanaged {
            return Err(format!("{name} is unmanaged, so it cannot be deleted."));
        }
        let operation = BlockingOperation::MeasureDelete(name.to_string());
        Ok(AppRequest::Block(operation))
    }

    pub fn rename(&self) -> Result<Session, String> {
        self.writable()?;
        let row = self.selected()?;
        let unmanaged = || format!("{} is unmanaged, so it cannot be renamed.", row.name());
        row.session().cloned().ok_or_else(unmanaged)
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

    pub fn merge(&self) -> Result<AppRequest, String> {
        self.writable()?;
        let row = self.selected()?;
        let name = row.name();
        match row.state() {
            SessionState::Waiting => Ok(AppRequest::Block(BlockingOperation::Merge(
                name.to_string(),
            ))),
            SessionState::Unmanaged => {
                Err(format!("{name} is unmanaged, so m cannot merge into it."))
            }
            state => Err(format!(
                "{name} is {}, m only merges into a session waiting at its prompt.",
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyAction {
    Request(AppRequest),
    Move(isize),
    SelectFirst,
    SelectLast,
    Fold,
    JumpToRepo(usize),
    Open(DialogOpening),
    Reload,
    Refuse(String),
    Ignore,
}

impl From<Result<KeyAction, String>> for KeyAction {
    fn from(action: Result<KeyAction, String>) -> Self {
        action.unwrap_or_else(KeyAction::Refuse)
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
    fn a_waiting_session_merges() {
        let row = GateFixture::row(SessionState::Waiting);

        assert_eq!(
            ActionGate::new(Some(&row), None).merge(),
            Ok(AppRequest::Block(BlockingOperation::Merge(
                "fix-auth".to_string()
            )))
        );
    }

    #[test]
    fn a_session_away_from_its_prompt_cannot_merge() {
        for state in [
            SessionState::Working,
            SessionState::Exited,
            SessionState::Stopped,
        ] {
            let row = GateFixture::row(state);

            assert_eq!(
                ActionGate::new(Some(&row), None).merge(),
                Err(format!(
                    "fix-auth is {}, m only merges into a session waiting at its prompt.",
                    state.label()
                ))
            );
        }
    }

    #[test]
    fn an_unmanaged_session_cannot_merge() {
        let stranger = ServiceFixture::session("mystery");
        let row = SessionSummary::unmanaged(UnmanagedSession::new(
            "mystery",
            ServiceFixture::waiting(&stranger).without_metadata(),
        ));

        assert_eq!(
            ActionGate::new(Some(&row), None).merge(),
            Err("mystery is unmanaged, so m cannot merge into it.".to_string())
        );
    }

    #[test]
    fn a_read_only_tui_refuses_a_merge_with_the_config_error() {
        let row = GateFixture::row(SessionState::Waiting);
        let error = GateFixture::config_error();

        let refusal = ActionGate::new(Some(&row), Some(&error))
            .merge()
            .unwrap_err();

        assert!(refusal.starts_with("config error, running read-only: "));
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
    fn n_in_a_read_only_tui_is_refused_with_the_config_error() {
        let error = GateFixture::config_error();
        let key = KeyEvent::from(KeyCode::Char('n'));

        let action = ActionGate::new(None, Some(&error)).action_for(&key);

        assert!(
            matches!(action, KeyAction::Refuse(refusal) if refusal.starts_with("config error"))
        );
    }

    #[test]
    fn n_opens_the_new_session_dialog_without_a_selection() {
        let key = KeyEvent::from(KeyCode::Char('n'));

        assert_eq!(
            ActionGate::new(None, None).action_for(&key),
            KeyAction::Open(DialogOpening::NewSession)
        );
    }

    #[test]
    fn vim_keys_and_arrows_move_the_selection() {
        let gate = ActionGate::new(None, None);
        let action = |code| gate.action_for(&KeyEvent::from(code));

        assert_eq!(action(KeyCode::Char('l')), KeyAction::Move(1));
        assert_eq!(action(KeyCode::Right), KeyAction::Move(1));
        assert_eq!(action(KeyCode::Char('h')), KeyAction::Move(-1));
        assert_eq!(action(KeyCode::Left), KeyAction::Move(-1));
    }

    #[test]
    fn capital_g_selects_the_last_session() {
        let key = KeyEvent::from(KeyCode::Char('G'));

        assert_eq!(
            ActionGate::new(None, None).action_for(&key),
            KeyAction::SelectLast
        );
    }

    #[test]
    fn d_measures_what_deleting_the_selected_session_would_lose() {
        let row = GateFixture::row(SessionState::Waiting);
        let key = KeyEvent::from(KeyCode::Char('d'));

        assert_eq!(
            ActionGate::new(Some(&row), None).action_for(&key),
            KeyAction::Request(AppRequest::Block(BlockingOperation::MeasureDelete(
                "fix-auth".to_string()
            )))
        );
    }

    #[test]
    fn d_in_a_read_only_tui_is_refused() {
        let row = GateFixture::row(SessionState::Waiting);
        let error = GateFixture::config_error();

        assert!(ActionGate::new(Some(&row), Some(&error)).delete().is_err());
    }

    #[test]
    fn an_unmanaged_session_cannot_be_deleted() {
        let stranger = ServiceFixture::session("mystery");
        let row = SessionSummary::unmanaged(UnmanagedSession::new(
            "mystery",
            ServiceFixture::waiting(&stranger).without_metadata(),
        ));

        assert_eq!(
            ActionGate::new(Some(&row), None).delete(),
            Err("mystery is unmanaged, so it cannot be deleted.".to_string())
        );
    }

    #[test]
    fn capital_r_opens_the_rename_dialog_for_the_selected_session() {
        let row = GateFixture::row(SessionState::Waiting);
        let key = KeyEvent::from(KeyCode::Char('R'));

        assert_eq!(
            ActionGate::new(Some(&row), None).action_for(&key),
            KeyAction::Open(DialogOpening::Rename(Box::new(
                row.session().unwrap().clone()
            )))
        );
    }

    #[test]
    fn rename_in_a_read_only_tui_and_of_an_unmanaged_session_is_refused() {
        let error = GateFixture::config_error();
        let managed = GateFixture::row(SessionState::Waiting);
        let stranger = ServiceFixture::session("mystery");
        let unmanaged = SessionSummary::unmanaged(UnmanagedSession::new(
            "mystery",
            ServiceFixture::waiting(&stranger).without_metadata(),
        ));

        assert!(
            ActionGate::new(Some(&managed), Some(&error))
                .rename()
                .is_err()
        );
        assert_eq!(
            ActionGate::new(Some(&unmanaged), None).rename(),
            Err("mystery is unmanaged, so it cannot be renamed.".to_string())
        );
    }

    #[test]
    fn enter_on_a_heading_folds_it() {
        let key = KeyEvent::from(KeyCode::Enter);

        assert_eq!(
            ActionGate::on_heading(None).action_for(&key),
            KeyAction::Fold
        );
    }

    #[test]
    fn enter_on_a_session_attaches() {
        let row = GateFixture::row(SessionState::Waiting);
        let key = KeyEvent::from(KeyCode::Enter);

        assert_eq!(
            ActionGate::new(Some(&row), None).action_for(&key),
            KeyAction::Request(AppRequest::Attach("fix-auth".to_string()))
        );
    }

    #[test]
    fn space_folds() {
        let key = KeyEvent::from(KeyCode::Char(' '));

        assert_eq!(
            ActionGate::on_heading(None).action_for(&key),
            KeyAction::Fold
        );
    }

    #[test]
    fn a_digit_jumps_to_that_repo() {
        let gate = ActionGate::new(None, None);
        let action = |character| gate.action_for(&KeyEvent::from(KeyCode::Char(character)));

        assert_eq!(action('3'), KeyAction::JumpToRepo(3));
        assert_eq!(action('0'), KeyAction::Ignore);
    }

    #[test]
    fn a_heading_refuses_the_session_actions() {
        let gate = ActionGate::on_heading(None);
        let refusal = |character| gate.action_for(&KeyEvent::from(KeyCode::Char(character)));

        for character in ['d', 'R', 's', 't'] {
            assert_eq!(
                refusal(character),
                KeyAction::Refuse("No session is selected.".to_string())
            );
        }
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
