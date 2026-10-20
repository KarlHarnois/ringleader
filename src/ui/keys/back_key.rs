use crossterm::event::KeyEvent;

use super::control_key::ControlKey;

pub struct BackKey;

impl BackKey {
    const KEY: ControlKey = ControlKey::new('q');

    pub fn matches(key: &KeyEvent) -> bool {
        Self::KEY.matches(key)
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEventKind, KeyEventState, KeyModifiers};

    use super::*;

    #[test]
    fn control_q_is_the_back_key() {
        let key = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);

        assert!(BackKey::matches(&key));
    }

    #[test]
    fn a_plain_q_is_not_the_back_key() {
        let key = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);

        assert!(!BackKey::matches(&key));
    }

    #[test]
    fn releasing_control_q_is_not_a_press() {
        let key = KeyEvent::new_with_kind_and_state(
            KeyCode::Char('q'),
            KeyModifiers::CONTROL,
            KeyEventKind::Release,
            KeyEventState::NONE,
        );

        assert!(!BackKey::matches(&key));
    }
}
