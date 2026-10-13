use crossterm::event::{KeyCode, KeyEvent};

use super::action_gate::{ActionGate, KeyAction};

#[derive(Debug, Default)]
pub struct KeySequence {
    pending_g: bool,
}

impl KeySequence {
    pub fn handle_key(&mut self, key: &KeyEvent, gate: &ActionGate) -> KeyAction {
        let pending_g = std::mem::take(&mut self.pending_g);
        match key.code {
            KeyCode::Char('g') if pending_g => KeyAction::SelectFirst,
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
            [KeyAction::Ignore, KeyAction::SelectFirst]
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
            [KeyAction::Ignore, KeyAction::Move(1)]
        );
    }

    #[test]
    fn g_j_g_leaves_a_sequence_pending() {
        assert_eq!(
            SequenceFixture::actions("gjg"),
            [KeyAction::Ignore, KeyAction::Move(1), KeyAction::Ignore]
        );
    }
}
