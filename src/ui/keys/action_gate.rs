use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::back_key::BackKey;
use crate::application::ports::ConfigError;
use crate::domain::{RepoGroup, Session, SessionState, SessionSummary};
use crate::ui::app::AppRequest;
use crate::ui::blocking_operation::{BlockingOperation, DeleteStep, RestartTarget};
use crate::ui::dialogs::DialogOpening;

pub struct ActionGate<'a> {
    selected: Option<&'a SessionSummary>,
    config_error: Option<&'a ConfigError>,
    heading: Option<&'a RepoGroup>,
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
            heading: None,
        }
    }

    pub fn on_heading(heading: &'a RepoGroup, config_error: Option<&'a ConfigError>) -> Self {
        Self {
            selected: None,
            config_error,
            heading: Some(heading),
        }
    }

    pub fn action_for(&self, key: &KeyEvent) -> KeyAction {
        if BackKey::matches(key) {
            return KeyAction::Ignore;
        }
        Self::screen_action(key)
            .or_else(|| Self::selection_move(key.code))
            .or_else(|| Self::repo_action(key.code))
            .or_else(|| self.access_action(key.code))
            .unwrap_or_else(|| self.session_change(key.code))
    }

    fn screen_action(key: &KeyEvent) -> Option<KeyAction> {
        Self::plain_screen_action(key.code).or_else(|| Self::control_screen_action(key))
    }

    fn plain_screen_action(code: KeyCode) -> Option<KeyAction> {
        let action = match code {
            KeyCode::Char('q') => KeyAction::Request(AppRequest::Quit),
            KeyCode::Char('r') => KeyAction::Reload,
            KeyCode::Char('?') => KeyAction::Open(DialogOpening::Help),
            KeyCode::Char('u') => KeyAction::Open(DialogOpening::Usage),
            KeyCode::Char('/') => KeyAction::FindSession,
            _ => return None,
        };
        Some(action)
    }

    fn control_screen_action(key: &KeyEvent) -> Option<KeyAction> {
        if !key.modifiers.contains(KeyModifiers::CONTROL) {
            return None;
        }
        let action = match key.code {
            KeyCode::Char('k') => KeyAction::FindSession,
            KeyCode::Char('f') => KeyAction::ToggleFilter,
            _ => return None,
        };
        Some(action)
    }

    fn selection_move(code: KeyCode) -> Option<KeyAction> {
        let navigation = match code {
            KeyCode::Down | KeyCode::Right | KeyCode::Char('j' | 'l') => Navigation::Move(1),
            KeyCode::Up | KeyCode::Left | KeyCode::Char('k' | 'h') => Navigation::Move(-1),
            KeyCode::Char('G') => Navigation::SelectLast,
            KeyCode::Char('H') => Navigation::SelectHigh,
            KeyCode::Char('M') => Navigation::SelectMiddle,
            KeyCode::Char('L') => Navigation::SelectLow,
            _ => return None,
        };
        Some(KeyAction::Navigate(navigation))
    }

    fn repo_action(code: KeyCode) -> Option<KeyAction> {
        let action = match code {
            KeyCode::Char(' ') => KeyAction::Fold,
            KeyCode::Char(digit @ '1'..='9') => {
                KeyAction::Navigate(Navigation::JumpToRepo(Self::repo_number(digit)))
            }
            _ => return None,
        };
        Some(action)
    }

    fn access_action(&self, code: KeyCode) -> Option<KeyAction> {
        let action = match code {
            KeyCode::Enter => self.enter(),
            KeyCode::Char('t') => KeyAction::from(self.shell().map(KeyAction::Request)),
            KeyCode::Char('T') => KeyAction::from(self.terminal().map(KeyAction::OpenOnDesktop)),
            KeyCode::Char('o') => self.open_in_browser(),
            _ => return None,
        };
        Some(action)
    }

    fn session_change(&self, code: KeyCode) -> KeyAction {
        match code {
            KeyCode::Char('f') => KeyAction::from(self.toggle_favorite()),
            KeyCode::Char('n' | 'R') => self.session_dialog(code),
            KeyCode::Char('s' | 'S' | 'd' | 'p') => self.session_command(code),
            _ => KeyAction::Ignore,
        }
    }

    fn session_dialog(&self, code: KeyCode) -> KeyAction {
        let action = match code {
            KeyCode::Char('n') => self
                .writable()
                .map(|()| KeyAction::Open(DialogOpening::NewSession(self.selected_repo()))),
            KeyCode::Char('R') => self
                .rename()
                .map(|session| KeyAction::Open(DialogOpening::Rename(Box::new(session)))),
            _ => return KeyAction::Ignore,
        };
        KeyAction::from(action)
    }

    fn session_command(&self, code: KeyCode) -> KeyAction {
        let request = match code {
            KeyCode::Char('s') => self.restart(),
            KeyCode::Char('S') => self.restart_all(),
            KeyCode::Char('d') => self.delete(),
            KeyCode::Char('p') => self.pull(),
            _ => return KeyAction::Ignore,
        };
        KeyAction::from(request.map(KeyAction::Request))
    }

    fn repo_number(digit: char) -> usize {
        let number = digit.to_digit(10).unwrap_or_default();
        usize::try_from(number).unwrap_or_default()
    }

    fn enter(&self) -> KeyAction {
        if self.heading.is_some() {
            return KeyAction::Fold;
        }
        KeyAction::from(self.attach().map(KeyAction::Request))
    }

    pub fn attach(&self) -> Result<AppRequest, String> {
        Ok(AppRequest::Attach(self.selected()?.name().to_string()))
    }

    pub fn shell(&self) -> Result<AppRequest, String> {
        self.managed_name("t cannot open a shell in it")
            .map(AppRequest::Shell)
    }

    pub fn terminal(&self) -> Result<DesktopOpening, String> {
        if let Some(group) = self.heading {
            return Ok(DesktopOpening::RepoTerminal(group.clone()));
        }
        self.managed_name("T cannot open a terminal in it")
            .map(DesktopOpening::SessionTerminal)
    }

    pub fn open_in_browser(&self) -> KeyAction {
        if let Some(group) = self.heading {
            let operation = BlockingOperation::OpenRepoPage(group.clone());
            return KeyAction::Request(AppRequest::Block(operation));
        }
        let name = self.selected().map(|row| row.name().to_string());
        KeyAction::from(
            name.map(|name| KeyAction::OpenOnDesktop(DesktopOpening::PullRequest(name))),
        )
    }

    fn managed_name(&self, refusal: &str) -> Result<String, String> {
        let row = self.selected()?;
        let name = row.name();
        match row.state() {
            SessionState::Unmanaged => Err(format!("{name} is unmanaged, so {refusal}.")),
            _ => Ok(name.to_string()),
        }
    }

    pub fn delete(&self) -> Result<AppRequest, String> {
        self.writable()?;
        let row = self.selected()?;
        let name = row.name();
        if row.state() == SessionState::Unmanaged {
            return Err(format!("{name} is unmanaged, so it cannot be deleted."));
        }
        let operation = BlockingOperation::Delete(DeleteStep::Measure(name.to_string()));
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
            state if state.allows_restart() => Ok(AppRequest::Block(BlockingOperation::Restart(
                RestartTarget::One(name.to_string()),
            ))),
            SessionState::Unmanaged => {
                Err(format!("{name} is unmanaged, so it cannot be restarted."))
            }
            state => Err(format!(
                "{name} is {}, so it cannot be restarted.",
                state.label()
            )),
        }
    }

    pub fn toggle_favorite(&self) -> Result<KeyAction, String> {
        self.writable()?;
        let row = self.selected()?;
        if !row.can_be_favorited() {
            return Err(format!(
                "{} is unmanaged, so it cannot be favorited.",
                row.name()
            ));
        }
        Ok(KeyAction::Request(AppRequest::Block(
            BlockingOperation::ToggleFavorite(row.name().to_string()),
        )))
    }

    pub fn restart_all(&self) -> Result<AppRequest, String> {
        self.writable()?;
        Ok(AppRequest::Block(BlockingOperation::Restart(
            RestartTarget::All,
        )))
    }

    pub fn pull(&self) -> Result<AppRequest, String> {
        self.writable()?;
        let row = self.selected()?;
        let name = row.name();
        if row.session().is_some_and(|session| !session.allows_pull()) {
            return Err(format!(
                "{name} runs in the main checkout, so p cannot pull into it."
            ));
        }
        match row.state() {
            SessionState::Waiting => Ok(AppRequest::Block(BlockingOperation::Merge(
                name.to_string(),
            ))),
            SessionState::Unmanaged => {
                Err(format!("{name} is unmanaged, so p cannot pull into it."))
            }
            state => Err(format!(
                "{name} is {}, p only pulls into a session waiting at its prompt.",
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

    fn selected_repo(&self) -> Option<PathBuf> {
        let group = self
            .heading
            .cloned()
            .or_else(|| self.selected.map(RepoGroup::of))?;
        group.path().map(Path::to_path_buf)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyAction {
    Request(AppRequest),
    Navigate(Navigation),
    Fold,
    Open(DialogOpening),
    FindSession,
    ToggleFilter,
    Reload,
    OpenOnDesktop(DesktopOpening),
    Refuse(String),
    Ignore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopOpening {
    SessionTerminal(String),
    RepoTerminal(RepoGroup),
    PullRequest(String),
}

impl From<Result<KeyAction, String>> for KeyAction {
    fn from(action: Result<KeyAction, String>) -> Self {
        action.unwrap_or_else(KeyAction::Refuse)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Navigation {
    Move(isize),
    SelectFirst,
    SelectLast,
    SelectHigh,
    SelectMiddle,
    SelectLow,
    JumpToRepo(usize),
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

        fn unmanaged_row() -> SessionSummary {
            let stranger = ServiceFixture::session("mystery");
            SessionSummary::unmanaged(UnmanagedSession::new(
                "mystery",
                ServiceFixture::waiting(&stranger).without_metadata(),
            ))
        }

        fn heading() -> RepoGroup {
            ServiceFixture::repo_group("/repos/alpha")
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
                    RestartTarget::One("fix-auth".to_string())
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
    fn shift_s_asks_to_restart_every_exited_or_stopped_session() {
        let row = GateFixture::row(SessionState::Waiting);
        let gate = ActionGate::new(Some(&row), None);

        assert_eq!(
            gate.action_for(&KeyEvent::from(KeyCode::Char('S'))),
            KeyAction::Request(AppRequest::Block(BlockingOperation::Restart(
                RestartTarget::All
            )))
        );
    }

    #[test]
    fn shift_s_on_a_heading_still_asks_to_restart_every_session() {
        let heading = GateFixture::heading();
        let gate = ActionGate::on_heading(&heading, None);

        assert_eq!(
            gate.action_for(&KeyEvent::from(KeyCode::Char('S'))),
            KeyAction::Request(AppRequest::Block(BlockingOperation::Restart(
                RestartTarget::All
            )))
        );
    }

    #[test]
    fn a_read_only_tui_refuses_a_restart_of_every_session_with_the_config_error() {
        let error = GateFixture::config_error();

        let refusal = ActionGate::new(None, Some(&error))
            .restart_all()
            .unwrap_err();

        assert!(refusal.starts_with("config error, running read-only: "));
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
    fn a_waiting_session_pulls() {
        let row = GateFixture::row(SessionState::Waiting);

        assert_eq!(
            ActionGate::new(Some(&row), None).pull(),
            Ok(AppRequest::Block(BlockingOperation::Merge(
                "fix-auth".to_string()
            )))
        );
    }

    #[test]
    fn a_session_away_from_its_prompt_cannot_pull() {
        for state in [
            SessionState::Working,
            SessionState::Exited,
            SessionState::Stopped,
        ] {
            let row = GateFixture::row(state);

            assert_eq!(
                ActionGate::new(Some(&row), None).pull(),
                Err(format!(
                    "fix-auth is {}, p only pulls into a session waiting at its prompt.",
                    state.label()
                ))
            );
        }
    }

    #[test]
    fn a_session_in_the_main_checkout_cannot_pull() {
        let in_main_checkout = ServiceFixture::main_checkout_session("fix-auth");
        let row = SessionSummary::managed(in_main_checkout, SessionState::Waiting, false);

        assert_eq!(
            ActionGate::new(Some(&row), None).pull(),
            Err("fix-auth runs in the main checkout, so p cannot pull into it.".to_string())
        );
    }

    #[test]
    fn an_unmanaged_session_cannot_pull() {
        let row = GateFixture::unmanaged_row();

        assert_eq!(
            ActionGate::new(Some(&row), None).pull(),
            Err("mystery is unmanaged, so p cannot pull into it.".to_string())
        );
    }

    #[test]
    fn a_read_only_tui_refuses_a_pull_with_the_config_error() {
        let row = GateFixture::row(SessionState::Waiting);
        let error = GateFixture::config_error();

        let refusal = ActionGate::new(Some(&row), Some(&error))
            .pull()
            .unwrap_err();

        assert!(refusal.starts_with("config error, running read-only: "));
    }

    #[test]
    fn an_unmanaged_session_opens_no_shell() {
        let row = GateFixture::unmanaged_row();

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
            KeyAction::Open(DialogOpening::NewSession(None))
        );
    }

    #[test]
    fn n_on_a_session_opens_the_dialog_in_its_repo() {
        let row = GateFixture::row(SessionState::Waiting);
        let key = KeyEvent::from(KeyCode::Char('n'));

        assert_eq!(
            ActionGate::new(Some(&row), None).action_for(&key),
            KeyAction::Open(DialogOpening::NewSession(Some(PathBuf::from(
                ServiceFixture::REPO
            ))))
        );
    }

    #[test]
    fn n_on_a_repo_heading_opens_the_dialog_in_that_repo() {
        let key = KeyEvent::from(KeyCode::Char('n'));

        assert_eq!(
            ActionGate::on_heading(&GateFixture::heading(), None).action_for(&key),
            KeyAction::Open(DialogOpening::NewSession(Some(PathBuf::from(
                "/repos/alpha"
            ))))
        );
    }

    #[test]
    fn n_on_an_other_row_opens_the_dialog_without_a_repo() {
        let row = GateFixture::unmanaged_row();
        let key = KeyEvent::from(KeyCode::Char('n'));

        assert_eq!(
            ActionGate::new(Some(&row), None).action_for(&key),
            KeyAction::Open(DialogOpening::NewSession(None))
        );
    }

    #[test]
    fn slash_and_control_k_open_the_session_finder() {
        let gate = ActionGate::new(None, None);
        let slash = KeyEvent::from(KeyCode::Char('/'));
        let control_k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL);

        assert_eq!(gate.action_for(&slash), KeyAction::FindSession);
        assert_eq!(gate.action_for(&control_k), KeyAction::FindSession);
    }

    #[test]
    fn a_plain_k_still_moves_the_selection_up() {
        let key = KeyEvent::from(KeyCode::Char('k'));

        assert_eq!(
            ActionGate::new(None, None).action_for(&key),
            KeyAction::Navigate(Navigation::Move(-1))
        );
    }

    #[test]
    fn only_a_plain_q_quits() {
        let gate = ActionGate::new(None, None);
        let plain_q = KeyEvent::from(KeyCode::Char('q'));
        let control_q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);

        assert_eq!(
            gate.action_for(&plain_q),
            KeyAction::Request(AppRequest::Quit)
        );
        assert_eq!(gate.action_for(&control_q), KeyAction::Ignore);
    }

    #[test]
    fn vim_keys_and_arrows_move_the_selection() {
        let gate = ActionGate::new(None, None);
        let action = |code| gate.action_for(&KeyEvent::from(code));
        let next = KeyAction::Navigate(Navigation::Move(1));
        let previous = KeyAction::Navigate(Navigation::Move(-1));

        assert_eq!(action(KeyCode::Char('l')), next);
        assert_eq!(action(KeyCode::Right), next);
        assert_eq!(action(KeyCode::Char('h')), previous);
        assert_eq!(action(KeyCode::Left), previous);
    }

    #[test]
    fn capital_g_selects_the_last_session() {
        let key = KeyEvent::from(KeyCode::Char('G'));

        assert_eq!(
            ActionGate::new(None, None).action_for(&key),
            KeyAction::Navigate(Navigation::SelectLast)
        );
    }

    #[test]
    fn capital_h_m_and_l_select_the_top_middle_and_bottom_rows_on_screen() {
        let gate = ActionGate::new(None, None);
        let action = |character| gate.action_for(&KeyEvent::from(KeyCode::Char(character)));

        assert_eq!(action('H'), KeyAction::Navigate(Navigation::SelectHigh));
        assert_eq!(action('M'), KeyAction::Navigate(Navigation::SelectMiddle));
        assert_eq!(action('L'), KeyAction::Navigate(Navigation::SelectLow));
    }

    #[test]
    fn d_measures_what_deleting_the_selected_session_would_lose() {
        let row = GateFixture::row(SessionState::Waiting);
        let key = KeyEvent::from(KeyCode::Char('d'));

        assert_eq!(
            ActionGate::new(Some(&row), None).action_for(&key),
            KeyAction::Request(AppRequest::Block(BlockingOperation::Delete(
                DeleteStep::Measure("fix-auth".to_string())
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
        let row = GateFixture::unmanaged_row();

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
        let unmanaged = GateFixture::unmanaged_row();

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
            ActionGate::on_heading(&GateFixture::heading(), None).action_for(&key),
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
            ActionGate::on_heading(&GateFixture::heading(), None).action_for(&key),
            KeyAction::Fold
        );
    }

    #[test]
    fn a_digit_jumps_to_that_repo() {
        let gate = ActionGate::new(None, None);
        let action = |character| gate.action_for(&KeyEvent::from(KeyCode::Char(character)));

        assert_eq!(action('3'), KeyAction::Navigate(Navigation::JumpToRepo(3)));
        assert_eq!(action('0'), KeyAction::Ignore);
    }

    #[test]
    fn a_heading_refuses_the_session_actions() {
        let heading = GateFixture::heading();
        let gate = ActionGate::on_heading(&heading, None);
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
        assert_eq!(gate.terminal(), Err("No session is selected.".to_string()));
        assert_eq!(
            gate.open_in_browser(),
            KeyAction::Refuse("No session is selected.".to_string())
        );
    }

    #[test]
    fn o_opens_the_pull_request_of_the_selected_session_even_read_only() {
        let error = GateFixture::config_error();
        let row = GateFixture::row(SessionState::Working);

        assert_eq!(
            ActionGate::new(Some(&row), Some(&error))
                .action_for(&KeyEvent::from(KeyCode::Char('o'))),
            KeyAction::OpenOnDesktop(DesktopOpening::PullRequest("fix-auth".to_string()))
        );
    }

    #[test]
    fn capital_t_opens_a_terminal_for_a_managed_session_in_every_state_even_read_only() {
        let error = GateFixture::config_error();
        let key = KeyEvent::from(KeyCode::Char('T'));

        for state in [
            SessionState::Working,
            SessionState::Waiting,
            SessionState::Exited,
            SessionState::Stopped,
        ] {
            let row = GateFixture::row(state);

            assert_eq!(
                ActionGate::new(Some(&row), Some(&error)).action_for(&key),
                KeyAction::OpenOnDesktop(DesktopOpening::SessionTerminal("fix-auth".to_string()))
            );
        }
    }

    #[test]
    fn capital_t_on_a_heading_opens_a_terminal_in_its_repo_even_read_only() {
        let heading = GateFixture::heading();
        let error = GateFixture::config_error();
        let key = KeyEvent::from(KeyCode::Char('T'));

        assert_eq!(
            ActionGate::on_heading(&heading, Some(&error)).action_for(&key),
            KeyAction::OpenOnDesktop(DesktopOpening::RepoTerminal(GateFixture::heading()))
        );
    }

    #[test]
    fn o_on_a_heading_looks_up_its_repo_page_behind_a_busy_message_even_read_only() {
        let heading = GateFixture::heading();
        let error = GateFixture::config_error();
        let key = KeyEvent::from(KeyCode::Char('o'));

        assert_eq!(
            ActionGate::on_heading(&heading, Some(&error)).action_for(&key),
            KeyAction::Request(AppRequest::Block(BlockingOperation::OpenRepoPage(
                GateFixture::heading()
            )))
        );
    }

    #[test]
    fn an_unmanaged_session_opens_no_terminal() {
        let row = GateFixture::unmanaged_row();

        assert_eq!(
            ActionGate::new(Some(&row), None).terminal(),
            Err("mystery is unmanaged, so T cannot open a terminal in it.".to_string())
        );
    }

    #[test]
    fn f_toggles_the_selected_session_favorite() {
        let row = GateFixture::row(SessionState::Waiting);

        assert_eq!(
            ActionGate::new(Some(&row), None).action_for(&KeyEvent::from(KeyCode::Char('f'))),
            KeyAction::Request(AppRequest::Block(BlockingOperation::ToggleFavorite(
                "fix-auth".to_string()
            )))
        );
    }

    #[test]
    fn f_without_a_selection_is_refused() {
        assert_eq!(
            ActionGate::new(None, None).action_for(&KeyEvent::from(KeyCode::Char('f'))),
            KeyAction::Refuse("No session is selected.".to_string())
        );
    }

    #[test]
    fn an_unmanaged_session_cannot_be_favorited() {
        let row = GateFixture::unmanaged_row();

        assert_eq!(
            ActionGate::new(Some(&row), None).action_for(&KeyEvent::from(KeyCode::Char('f'))),
            KeyAction::Refuse("mystery is unmanaged, so it cannot be favorited.".to_string())
        );
    }

    #[test]
    fn a_read_only_tui_refuses_to_toggle_a_favorite() {
        let row = GateFixture::row(SessionState::Waiting);
        let error = GateFixture::config_error();

        let refusal = ActionGate::new(Some(&row), Some(&error))
            .toggle_favorite()
            .unwrap_err();

        assert!(refusal.starts_with("config error, running read-only: "));
    }

    #[test]
    fn control_f_toggles_the_filter_even_read_only() {
        let error = GateFixture::config_error();
        let key = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL);

        assert_eq!(
            ActionGate::new(None, Some(&error)).action_for(&key),
            KeyAction::ToggleFilter
        );
    }
}
