use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

pub struct ControlKey {
    character: char,
}

impl ControlKey {
    pub const fn new(character: char) -> Self {
        Self { character }
    }

    pub fn matches(&self, key: &KeyEvent) -> bool {
        let pressed = key.kind != KeyEventKind::Release;
        pressed
            && key.code == KeyCode::Char(self.character)
            && key.modifiers == KeyModifiers::CONTROL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_control_key_matches_only_its_own_character_with_control() {
        let control_c = ControlKey::new('c');

        let matched = [
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
        ]
        .map(|key| control_c.matches(&key));

        assert_eq!(matched, [true, false, false]);
    }
}
