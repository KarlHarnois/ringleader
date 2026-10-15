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

    struct SequenceFixture;

    impl SequenceFixture {
        fn actions(characters: &str) -> Vec<KeyAction> {
            let gate = ActionGate::new(None, None);
            let mut sequence = KeySequence::default();
            characters
                .chars()
                .map(|character| KeyEvent::from(KeyCode::Char(character)))
                .map(|key| sequence.handle_key(&key, &gate))
                .collect()
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
}
