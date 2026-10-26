use crossterm::event::{KeyCode, KeyEvent};

use super::action_gate::{ActionGate, KeyAction, VisualChange};
use super::back_key::BackKey;
use super::key_map::KeyMap;
use crate::ui::app::AppRequest;
use crate::ui::batch_operation::{BatchOperation, DeleteSessionsStep};
use crate::ui::blocking_operation::BlockingOperation;

pub struct VisualGate<'a> {
    gate: ActionGate<'a>,
    names: Vec<String>,
}

impl<'a> VisualGate<'a> {
    const OPEN_IN_BROWSER: &'static str = "gx";
    const ENTER: &'static str = "Enter";

    pub fn new(gate: ActionGate<'a>, names: Vec<String>) -> Self {
        Self { gate, names }
    }

    fn mode_change(code: KeyCode) -> Option<KeyAction> {
        match code {
            KeyCode::Esc | KeyCode::Char('v' | 'V') => Some(KeyAction::Visual(VisualChange::Leave)),
            _ => None,
        }
    }

    fn batch_action(&self, code: KeyCode) -> Option<KeyAction> {
        let names = self.names.clone();
        let operation = match code {
            KeyCode::Char('x') => BlockingOperation::Batch(BatchOperation::Stop(names)),
            KeyCode::Char('s') => BlockingOperation::Batch(BatchOperation::Restart(names)),
            KeyCode::Char('p') => BlockingOperation::Batch(BatchOperation::Merge(names)),
            KeyCode::Char('d') => {
                BlockingOperation::Batch(BatchOperation::Delete(DeleteSessionsStep::Measure(names)))
            }
            _ => return None,
        };
        Some(KeyAction::from(self.batch(operation)))
    }

    fn batch(&self, operation: BlockingOperation) -> Result<KeyAction, String> {
        self.gate.writable()?;
        if self.names.is_empty() {
            return Err(ActionGate::NO_SELECTION.to_string());
        }
        Ok(KeyAction::Request(AppRequest::Block(operation)))
    }

    fn single_session_refusal(code: KeyCode) -> Option<KeyAction> {
        match code {
            KeyCode::Enter => Some(Self::refusal(Self::ENTER)),
            KeyCode::Char(key @ ('t' | 'T' | 'o' | 'R')) => Some(Self::refusal(key)),
            _ => None,
        }
    }

    fn refusal(key: impl std::fmt::Display) -> KeyAction {
        KeyAction::Refuse(format!(
            "{key} acts on one session. Press Esc to leave visual mode."
        ))
    }
}

impl KeyMap for VisualGate<'_> {
    fn action_for(&self, key: &KeyEvent) -> KeyAction {
        if BackKey::matches(key) {
            return KeyAction::Ignore;
        }
        Self::mode_change(key.code)
            .or_else(|| self.batch_action(key.code))
            .or_else(|| Self::single_session_refusal(key.code))
            .unwrap_or_else(|| self.gate.action_for(key))
    }

    fn open_in_browser(&self) -> KeyAction {
        Self::refusal(Self::OPEN_IN_BROWSER)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::application::ports::ConfigError;
    use crate::test_support::ServiceFixture;
    use crate::ui::dialogs::DialogOpening;
    use crate::ui::keys::action_gate::Navigation;

    struct VisualFixture;

    impl VisualFixture {
        fn names() -> Vec<String> {
            ["alpha", "bravo"].map(String::from).to_vec()
        }

        fn action(names: Vec<String>, code: KeyCode) -> KeyAction {
            VisualGate::new(ActionGate::new(None, None), names).action_for(&KeyEvent::from(code))
        }

        fn press(character: char) -> KeyAction {
            Self::action(Self::names(), KeyCode::Char(character))
        }

        fn block(operation: BlockingOperation) -> KeyAction {
            KeyAction::Request(AppRequest::Block(operation))
        }
    }

    #[test]
    fn x_s_and_p_act_on_every_session_in_the_range() {
        let names = VisualFixture::names();

        assert_eq!(
            VisualFixture::press('x'),
            VisualFixture::block(BlockingOperation::Batch(BatchOperation::Stop(
                names.clone()
            )))
        );
        assert_eq!(
            VisualFixture::press('s'),
            VisualFixture::block(BlockingOperation::Batch(BatchOperation::Restart(
                names.clone()
            )))
        );
        assert_eq!(
            VisualFixture::press('p'),
            VisualFixture::block(BlockingOperation::Batch(BatchOperation::Merge(names)))
        );
    }

    #[test]
    fn x_on_a_heading_stops_the_range_instead_of_every_session_of_the_repo() {
        let heading = ServiceFixture::repo_group(ServiceFixture::REPO);
        let gate = VisualGate::new(
            ActionGate::on_heading(&heading, None),
            VisualFixture::names(),
        );

        assert_eq!(
            gate.action_for(&KeyEvent::from(KeyCode::Char('x'))),
            VisualFixture::block(BlockingOperation::Batch(BatchOperation::Stop(
                VisualFixture::names()
            )))
        );
    }

    #[test]
    fn d_measures_what_deleting_the_range_would_lose() {
        assert_eq!(
            VisualFixture::press('d'),
            VisualFixture::block(BlockingOperation::Batch(BatchOperation::Delete(
                DeleteSessionsStep::Measure(VisualFixture::names())
            )))
        );
    }

    #[test]
    fn esc_v_and_capital_v_leave_visual_mode() {
        for code in [KeyCode::Esc, KeyCode::Char('v'), KeyCode::Char('V')] {
            assert_eq!(
                VisualFixture::action(VisualFixture::names(), code),
                KeyAction::Visual(VisualChange::Leave)
            );
        }
    }

    #[test]
    fn movement_keys_extend_the_range() {
        assert_eq!(
            VisualFixture::press('j'),
            KeyAction::Navigate(Navigation::Move(1))
        );
    }

    #[test]
    fn a_key_for_one_session_is_refused_with_how_to_leave() {
        assert_eq!(
            VisualFixture::action(VisualFixture::names(), KeyCode::Enter),
            KeyAction::Refuse(
                "Enter acts on one session. Press Esc to leave visual mode.".to_string()
            )
        );
    }

    #[test]
    fn a_letter_for_one_session_is_refused_by_its_letter() {
        assert_eq!(
            VisualFixture::press('t'),
            KeyAction::Refuse("t acts on one session. Press Esc to leave visual mode.".to_string())
        );
    }

    #[test]
    fn an_unbound_key_is_ignored_as_in_normal_mode() {
        assert_eq!(VisualFixture::press('z'), KeyAction::Ignore);
    }

    #[test]
    fn keys_that_never_needed_a_selection_keep_their_meaning() {
        assert_eq!(
            VisualFixture::press('S'),
            VisualFixture::block(BlockingOperation::Batch(BatchOperation::RestartAll))
        );
        assert_eq!(
            VisualFixture::press('n'),
            KeyAction::Open(DialogOpening::NewSession(None))
        );
    }

    #[test]
    fn a_range_without_a_session_refuses_the_action() {
        assert_eq!(
            VisualFixture::action(Vec::new(), KeyCode::Char('x')),
            KeyAction::Refuse(ActionGate::NO_SELECTION.to_string())
        );
    }

    #[test]
    fn a_read_only_tui_refuses_the_action_with_the_config_error() {
        let error = ConfigError::Parse {
            path: PathBuf::from("/config/ringleader/config.toml"),
            detail: "expected `=`".to_string(),
        };
        let gate = VisualGate::new(ActionGate::new(None, Some(&error)), VisualFixture::names());

        let action = gate.action_for(&KeyEvent::from(KeyCode::Char('x')));

        assert!(
            matches!(action, KeyAction::Refuse(refusal) if refusal.starts_with("config error"))
        );
    }
}
