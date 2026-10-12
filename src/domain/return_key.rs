use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnKey {
    modifiers: Vec<Modifier>,
    key: String,
}

impl ReturnKey {
    pub const DEFAULT: &'static str = "C-q";
    const NAMED_KEYS: [&'static str; 30] = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "Escape",
        "Enter", "Tab", "BTab", "Space", "BSpace", "Home", "End", "IC", "DC", "PageUp", "PageDown",
        "PPage", "NPage", "Up", "Down", "Left", "Right",
    ];
    const REFUSED_CHARACTERS: [char; 3] = ['\'', '"', '\\'];

    pub fn parse(text: &str) -> Option<Self> {
        let mut rest = text;
        let mut modifiers = Vec::new();
        while let Some((modifier, after)) = Modifier::strip(rest) {
            modifiers.push(modifier);
            rest = after;
        }
        Self::is_key(rest).then(|| Self {
            modifiers,
            key: rest.to_string(),
        })
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

    fn displayed_key(&self) -> String {
        let is_control_character =
            self.modifiers.contains(&Modifier::Control) && self.key.chars().count() == 1;
        match is_control_character {
            true => self.key.to_uppercase(),
            false => self.key.clone(),
        }
    }

    fn tmux_name(&self) -> String {
        let prefixes: String = self.modifiers.iter().map(Modifier::prefix).collect();
        format!("{prefixes}{}", self.key)
    }

    fn is_key(text: &str) -> bool {
        let mut characters = text.chars();
        match (characters.next(), characters.next()) {
            (Some(single), None) => Self::is_plain_character(single),
            _ => Self::NAMED_KEYS.contains(&text),
        }
    }

    fn is_plain_character(character: char) -> bool {
        character.is_ascii_graphic() && !Self::REFUSED_CHARACTERS.contains(&character)
    }
}

impl Default for ReturnKey {
    fn default() -> Self {
        Self::parse(Self::DEFAULT).expect("the default return key parses")
    }
}

impl fmt::Display for ReturnKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.tmux_name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Modifier {
    Control,
    Meta,
    Shift,
}

impl Modifier {
    const ALL: [Self; 3] = [Self::Control, Self::Meta, Self::Shift];

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

    #[test]
    fn modifiers_and_single_characters_and_named_keys_parse() {
        for text in ["C-q", "M-x", "F12", "~", "}", "C-}", "C--", "S-F1", "C-M-x"] {
            assert!(ReturnKey::parse(text).is_some(), "{text} should parse");
        }
    }

    #[test]
    fn empty_bare_modifiers_quotes_backslashes_and_whitespace_are_refused() {
        for text in [
            "", "C-", "M-", "'", "\"", "\\", "C-'", " ", "C- ", "\n", "qq", "F13",
        ] {
            assert!(ReturnKey::parse(text).is_none(), "{text:?} should fail");
        }
    }

    #[test]
    fn the_tmux_spelling_is_single_quoted() {
        let key = ReturnKey::parse("C-}").unwrap();

        assert_eq!(key.tmux_spelling(), "'C-}'");
    }

    #[test]
    fn the_display_form_names_the_modifiers() {
        let shown = ["C-q", "M-x", "S-F1", "C-Escape", "~"]
            .map(|text| ReturnKey::parse(text).unwrap().display());

        assert_eq!(shown, ["Ctrl-Q", "Alt-x", "Shift-F1", "Ctrl-Escape", "~"]);
    }

    #[test]
    fn the_default_is_ctrl_q() {
        assert_eq!(ReturnKey::default().to_string(), "C-q");
    }
}
