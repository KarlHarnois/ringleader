use crossterm::event::{KeyCode, KeyEvent};

use super::action_gate::{ActionGate, KeyAction, Navigation};

#[derive(Debug, Default)]
pub struct KeySequence {
    pending_g: bool,
}

impl KeySequence {
    pub fn handle_key(&mut self, key: &KeyEvent, gate: &ActionGate) -> KeyAction {
        let pending_g = std::mem::take(&mut self.pending_g);
        match key.code {
            KeyCode::Char('g') if pending_g => KeyAction::Navigate(Navigation::SelectFirst),
            KeyCode::Char('x') if pending_g => gate.open_in_browser(),
            KeyCode::Char('g') => {
                self.pending_g = true;
                KeyAction::Ignore
            }
            _ => gate.action_for(key),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{SessionState, SessionSummary};
    use crate::test_support::ServiceFixture;
    use crate::ui::app::AppRequest;
    use crate::ui::blocking_operation::BlockingOperation;
    use crate::ui::keys::action_gate::DesktopOpening;

    struct SequenceFixture;

    impl SequenceFixture {
        fn actions(characters: &str) -> Vec<KeyAction> {
            Self::actions_with(&ActionGate::new(None, None), characters)
        }

        fn actions_on(row: &SessionSummary, characters: &str) -> Vec<KeyAction> {
            Self::actions_with(&ActionGate::new(Some(row), None), characters)
        }

        fn actions_with(gate: &ActionGate, characters: &str) -> Vec<KeyAction> {
            let mut sequence = KeySequence::default();
            characters
                .chars()
                .map(|character| KeyEvent::from(KeyCode::Char(character)))
                .map(|key| sequence.handle_key(&key, gate))
                .collect()
        }

        fn row() -> SessionSummary {
            SessionSummary::managed(
                ServiceFixture::session("fix-auth"),
                SessionState::Waiting,
                false,
            )
        }
    }

    #[test]
    fn gg_selects_the_first_session() {
        assert_eq!(
            SequenceFixture::actions("gg"),
            [
                KeyAction::Ignore,
                KeyAction::Navigate(Navigation::SelectFirst)
            ]
        );
    }

    #[test]
    fn a_lone_g_is_ignored() {
        assert_eq!(SequenceFixture::actions("g"), [KeyAction::Ignore]);
    }

    #[test]
    fn g_then_j_moves_by_one() {
        assert_eq!(
            SequenceFixture::actions("gj"),
            [KeyAction::Ignore, KeyAction::Navigate(Navigation::Move(1))]
        );
    }

    #[test]
    fn a_g_after_a_cancelled_one_waits_again() {
        assert_eq!(
            SequenceFixture::actions("gjgg"),
            [
                KeyAction::Ignore,
                KeyAction::Navigate(Navigation::Move(1)),
                KeyAction::Ignore,
                KeyAction::Navigate(Navigation::SelectFirst)
            ]
        );
    }

    #[test]
    fn gx_opens_the_pull_request_of_the_selected_session() {
        assert_eq!(
            SequenceFixture::actions_on(&SequenceFixture::row(), "gx"),
            [
                KeyAction::Ignore,
                KeyAction::OpenOnDesktop(DesktopOpening::PullRequest("fix-auth".to_string()))
            ]
        );
    }

    #[test]
    fn a_lone_x_stops_the_selected_session_instead_of_opening_its_pull_request() {
        assert_eq!(
            SequenceFixture::actions_on(&SequenceFixture::row(), "x"),
            [KeyAction::Request(AppRequest::Block(
                BlockingOperation::Stop("fix-auth".to_string())
            ))]
        );
    }
}
