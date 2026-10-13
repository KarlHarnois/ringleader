use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotKey {
    modifiers: Vec<Modifier>,
    key: String,
}

impl HotKey {
    const NAMED_KEYS: [&'static str; 28] = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "Escape",
        "Enter", "Tab", "BTab", "Space", "BSpace", "Home", "End", "IC", "DC", "PageUp", "PageDown",
        "Up", "Down", "Left", "Right",
    ];
    const ALIASES: [(&'static str, &'static str); 2] = [("PPage", "PageUp"), ("NPage", "PageDown")];
    const ARROWS: [(&'static str, &'static str); 4] =
        [("Up", "↑"), ("Down", "↓"), ("Left", "←"), ("Right", "→")];
    const UNMODIFIABLE_KEYS: [&'static str; 6] =
        ["Enter", "Tab", "BTab", "Escape", "BSpace", "Space"];
    const SPACE: &'static str = "Space";
    const REFUSED_CHARACTERS: [char; 3] = ['\'', '"', '\\'];
    const CONTROL_PUNCTUATION: [char; 3] = [']', '^', '_'];
    const LETTERS_READ_AS_TAB_AND_ENTER: [char; 2] = ['i', 'm'];
    const AGENT_KEYS: [&'static str; 13] = [
        "C-c", "C-d", "C-z", "C-j", "C-h", "Enter", "Escape", "Tab", "BTab", "BSpace", "Space",
        "Up", "Down",
    ];

    pub fn parse(text: &str) -> Option<Self> {
        let (modifiers, rest) = Modifier::split(text);
        let key = Self {
            key: Self::normalized(&modifiers, rest),
            modifiers,
        };
        key.is_reachable().then_some(key)
    }

    pub fn parse_valid<T: AsRef<str>>(texts: impl IntoIterator<Item = T>) -> Vec<Self> {
        texts
            .into_iter()
            .filter_map(|text| Self::parse(text.as_ref()))
            .collect()
    }

    pub fn shadows_agent_input(&self) -> bool {
        Self::AGENT_KEYS.contains(&self.tmux_name().as_str()) || self.is_typed_character()
    }

    pub fn tmux_spelling(&self) -> String {
        format!("'{}'", self.tmux_name())
    }

    pub fn display(&self) -> String {
        let key = self.displayed_key();
        let mut words: Vec<&str> = self.modifiers.iter().map(Modifier::word).collect();
        words.push(&key);
        words.join("-")
    }

    fn normalized(modifiers: &[Modifier], key: &str) -> String {
        let control_character = modifiers.contains(&Modifier::Control) && key.chars().count() == 1;
        match control_character {
            true => key.to_ascii_lowercase(),
            false => Self::unaliased(key).to_string(),
        }
    }

    fn unaliased(key: &str) -> &str {
        Self::ALIASES
            .iter()
            .find(|(alias, _)| *alias == key)
            .map_or(key, |(_, name)| name)
    }

    fn displayed_key(&self) -> String {
        match self.has(Modifier::Control) && self.single_character().is_some() {
            true => self.key.to_uppercase(),
            false => Self::arrow(&self.key).unwrap_or(&self.key).to_string(),
        }
    }

    fn arrow(key: &str) -> Option<&'static str> {
        Self::ARROWS
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, arrow)| *arrow)
    }

    fn is_typed_character(&self) -> bool {
        self.modifiers.is_empty() && self.single_character().is_some()
    }

    fn tmux_name(&self) -> String {
        let prefixes: String = self.modifiers.iter().map(Modifier::prefix).collect();
        format!("{prefixes}{}", self.key)
    }

    fn is_reachable(&self) -> bool {
        match self.single_character() {
            Some(character) => self.reaches_character(character),
            None => self.reaches_named_key(),
        }
    }

    fn reaches_character(&self, character: char) -> bool {
        let printable =
            character.is_ascii_graphic() && !Self::REFUSED_CHARACTERS.contains(&character);
        let control_letter = character.is_ascii_lowercase()
            && !Self::LETTERS_READ_AS_TAB_AND_ENTER.contains(&character);
        let sent_with_control = control_letter || Self::CONTROL_PUNCTUATION.contains(&character);
        let control_reaches = !self.has(Modifier::Control) || sent_with_control;
        printable && control_reaches && !self.has(Modifier::Shift)
    }

    fn reaches_named_key(&self) -> bool {
        let key = self.key.as_str();
        if !Self::NAMED_KEYS.contains(&key) {
            return false;
        }
        if !Self::UNMODIFIABLE_KEYS.contains(&key) {
            return true;
        }
        let control_reaches = !self.has(Modifier::Control) || key == Self::SPACE;
        control_reaches && !self.has(Modifier::Shift)
    }

    fn single_character(&self) -> Option<char> {
        let mut characters = self.key.chars();
        characters.next().filter(|_| characters.next().is_none())
    }

    fn has(&self, modifier: Modifier) -> bool {
        self.modifiers.contains(&modifier)
    }
}

impl fmt::Display for HotKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.tmux_name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Modifier {
    Control,
    Meta,
    Shift,
}

impl Modifier {
    const ALL: [Self; 3] = [Self::Control, Self::Meta, Self::Shift];

    fn split(text: &str) -> (Vec<Self>, &str) {
        let mut rest = text;
        let mut modifiers = Vec::new();
        while let Some((modifier, after)) = Self::strip(rest) {
            modifiers.push(modifier);
            rest = after;
        }
        modifiers.sort();
        modifiers.dedup();
        (modifiers, rest)
    }

    fn strip(text: &str) -> Option<(Self, &str)> {
        Self::ALL.into_iter().find_map(|modifier| {
            let rest = text.strip_prefix(modifier.prefix())?;
            (!rest.is_empty()).then_some((modifier, rest))
        })
    }

    fn prefix(&self) -> &'static str {
        match self {
            Self::Control => "C-",
            Self::Meta => "M-",
            Self::Shift => "S-",
        }
    }

    fn word(&self) -> &'static str {
        match self {
            Self::Control => "Ctrl",
            Self::Meta => "Alt",
            Self::Shift => "Shift",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct KeyFixture;

    impl KeyFixture {
        fn assert_parse(texts: &[&str]) {
            for text in texts {
                assert!(HotKey::parse(text).is_some(), "{text:?} should parse");
            }
        }

        fn assert_refused(texts: &[&str]) {
            for text in texts {
                assert!(HotKey::parse(text).is_none(), "{text:?} should fail");
            }
        }
    }

    #[test]
    fn modifiers_and_single_characters_and_named_keys_parse() {
        KeyFixture::assert_parse(&["C-q", "M-x", "F12", "~", "}", "M-}", "M-X", "C-M-x", "S-F1"]);
    }

    #[test]
    fn empty_bare_modifiers_quotes_backslashes_and_whitespace_are_refused() {
        KeyFixture::assert_refused(&[
            "", "C-", "M-", "'", "\"", "\\", "C-'", " ", "C- ", "\n", "qq", "F13",
        ]);
    }

    #[test]
    fn ctrl_takes_a_letter_or_a_character_terminals_send_with_ctrl() {
        KeyFixture::assert_parse(&["C-a", "C-z", "C-]", "C-^", "C-_", "C-Space", "C-Up", "C-F1"]);
        KeyFixture::assert_refused(&[
            "C-1", "C-2", "C-/", "C-?", "C-@", "C-[", "C-}", "C-~", "C-$", "C-M-}",
        ]);
    }

    #[test]
    fn ctrl_i_and_ctrl_m_are_refused_because_terminals_send_tab_and_enter_for_them() {
        KeyFixture::assert_refused(&["C-i", "C-m", "C-M-i", "C-M-m", "C-I", "C-M"]);
    }

    #[test]
    fn an_uppercase_letter_after_ctrl_means_the_same_key_as_the_lowercase_one() {
        let key = HotKey::parse("C-Q").unwrap();

        assert_eq!(key, HotKey::parse("C-q").unwrap());
    }

    #[test]
    fn modifier_order_and_page_key_aliases_name_the_same_key() {
        for (text, same) in [
            ("M-C-x", "C-M-x"),
            ("C-C-x", "C-x"),
            ("NPage", "PageDown"),
            ("C-PPage", "C-PageUp"),
        ] {
            assert_eq!(HotKey::parse(text), HotKey::parse(same), "{text}");
        }
    }

    #[test]
    fn shift_before_a_character_is_refused() {
        KeyFixture::assert_refused(&["S-x", "S-}", "M-S-x"]);
    }

    #[test]
    fn keys_terminals_send_without_modifiers_take_no_ctrl_or_shift() {
        KeyFixture::assert_parse(&["Enter", "Tab", "M-Enter", "BTab"]);
        KeyFixture::assert_refused(&[
            "C-Enter", "C-Tab", "C-Escape", "C-BSpace", "S-Tab", "S-Space",
        ]);
    }

    #[test]
    fn the_tmux_spelling_is_single_quoted() {
        let key = HotKey::parse("M-}").unwrap();

        assert_eq!(key.tmux_spelling(), "'M-}'");
    }

    #[test]
    fn the_display_form_names_the_modifiers() {
        let shown =
            ["C-q", "M-x", "S-F1", "C-Up", "~"].map(|text| HotKey::parse(text).unwrap().display());

        assert_eq!(shown, ["Ctrl-Q", "Alt-x", "Shift-F1", "Ctrl-↑", "~"]);
    }

    #[test]
    fn keys_agents_need_and_typed_characters_shadow_agent_input() {
        for text in ["C-c", "C-d", "Enter", "Escape", "~", "x"] {
            let key = HotKey::parse(text).unwrap();

            assert!(key.shadows_agent_input(), "{text}");
        }
    }

    #[test]
    fn the_default_keys_and_other_rare_keys_leave_agent_input_alone() {
        for text in ["C-q", "M-j", "M-Down", "M-k", "M-Up", "M-x", "F12", "M-}"] {
            assert!(
                !HotKey::parse(text).unwrap().shadows_agent_input(),
                "{text}"
            );
        }
    }
}
