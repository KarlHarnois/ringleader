use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Mode,
    Agent,
    Checkout,
}

impl Setting {
    pub const ALL: [Setting; 3] = [Self::Mode, Self::Agent, Self::Checkout];
    const SECOND_MODE_KEY: KeyCode = KeyCode::BackTab;

    pub fn for_key(key: &KeyEvent) -> Option<Self> {
        let chorded = Self::ALL
            .into_iter()
            .find(|setting| setting.key().matches(key));
        chorded.or_else(|| Self::second_mode_key(key))
    }

    pub fn key(self) -> AltChord {
        match self {
            Self::Mode => AltChord::new('j'),
            Self::Agent => AltChord::new('k'),
            Self::Checkout => AltChord::new('l'),
        }
    }

    fn second_mode_key(key: &KeyEvent) -> Option<Self> {
        (key.code == Self::SECOND_MODE_KEY).then_some(Self::Mode)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AltChord {
    character: char,
}

impl AltChord {
    const MODIFIER_WORD: &'static str = "Alt";

    pub const fn new(character: char) -> Self {
        Self { character }
    }

    pub fn matches(&self, key: &KeyEvent) -> bool {
        key.code == KeyCode::Char(self.character) && key.modifiers == KeyModifiers::ALT
    }

    pub fn label(&self) -> String {
        format!("{}-{}", Self::MODIFIER_WORD, self.character)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::HotKey;

    struct KeyFixture;

    impl KeyFixture {
        fn alt(character: char) -> KeyEvent {
            KeyEvent::new(KeyCode::Char(character), KeyModifiers::ALT)
        }
    }

    #[test]
    fn alt_j_alt_k_and_alt_l_change_the_mode_the_agent_and_the_checkout() {
        let settings =
            ['j', 'k', 'l'].map(|character| Setting::for_key(&KeyFixture::alt(character)));

        assert_eq!(
            settings,
            [
                Some(Setting::Mode),
                Some(Setting::Agent),
                Some(Setting::Checkout)
            ]
        );
    }

    #[test]
    fn shift_tab_is_a_second_mode_key() {
        let key = KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT);

        assert_eq!(Setting::for_key(&key), Some(Setting::Mode));
    }

    #[test]
    fn a_plain_j_and_a_control_j_change_no_setting() {
        let keys = [KeyModifiers::NONE, KeyModifiers::CONTROL]
            .map(|modifiers| KeyEvent::new(KeyCode::Char('j'), modifiers));

        assert_eq!(keys.map(|key| Setting::for_key(&key)), [None, None]);
    }

    #[test]
    fn the_keys_run_along_the_home_row_in_status_line_order() {
        assert_eq!(
            Setting::ALL.map(|setting| setting.key().label()),
            ["Alt-j", "Alt-k", "Alt-l"]
        );
    }

    #[test]
    fn the_chord_label_spells_alt_the_way_the_help_view_does() {
        assert_eq!(
            AltChord::new('j').label(),
            HotKey::parse("M-j").unwrap().display()
        );
    }
}
