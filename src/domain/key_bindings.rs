use std::iter::once;

use super::hot_key::HotKey;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBindings {
    return_key: HotKey,
    next_session_keys: Vec<HotKey>,
    previous_session_keys: Vec<HotKey>,
    shell_toggle_keys: Vec<HotKey>,
}

impl KeyBindings {
    pub const RETURN_KEY: &'static str = "C-q";
    pub const NEXT_SESSION_KEYS: [&'static str; 2] = ["M-j", "M-Down"];
    pub const PREVIOUS_SESSION_KEYS: [&'static str; 2] = ["M-k", "M-Up"];
    pub const SHELL_TOGGLE_KEYS: [&'static str; 1] = ["M-l"];
    pub const RETURN_KEY_SETTING: &'static str = "return_key";
    pub const NEXT_SESSION_SETTING: &'static str = "next_session_keys";
    pub const PREVIOUS_SESSION_SETTING: &'static str = "previous_session_keys";
    pub const SHELL_TOGGLE_SETTING: &'static str = "shell_toggle_keys";

    pub fn new(
        return_key: HotKey,
        next_session_keys: Vec<HotKey>,
        previous_session_keys: Vec<HotKey>,
    ) -> Self {
        Self {
            return_key,
            next_session_keys,
            previous_session_keys,
            shell_toggle_keys: Vec::new(),
        }
    }

    pub fn with_shell_toggle_keys(self, shell_toggle_keys: Vec<HotKey>) -> Self {
        Self {
            shell_toggle_keys,
            ..self
        }
    }

    #[cfg(test)]
    pub fn with_return_key(self, return_key: HotKey) -> Self {
        Self { return_key, ..self }
    }

    #[cfg(test)]
    pub fn with_next_session_keys(self, next_session_keys: Vec<HotKey>) -> Self {
        Self {
            next_session_keys,
            ..self
        }
    }

    #[cfg(test)]
    pub fn with_previous_session_keys(self, previous_session_keys: Vec<HotKey>) -> Self {
        Self {
            previous_session_keys,
            ..self
        }
    }

    pub fn return_key(&self) -> &HotKey {
        &self.return_key
    }

    pub fn next_session_keys(&self) -> &[HotKey] {
        &self.next_session_keys
    }

    pub fn previous_session_keys(&self) -> &[HotKey] {
        &self.previous_session_keys
    }

    pub fn shell_toggle_keys(&self) -> &[HotKey] {
        &self.shell_toggle_keys
    }

    pub fn all(&self) -> impl Iterator<Item = &HotKey> {
        self.by_setting().into_iter().map(|(_, key)| key)
    }

    pub fn by_setting(&self) -> Vec<(&'static str, &HotKey)> {
        once((Self::RETURN_KEY_SETTING, &self.return_key))
            .chain(Self::named(
                Self::NEXT_SESSION_SETTING,
                &self.next_session_keys,
            ))
            .chain(Self::named(
                Self::PREVIOUS_SESSION_SETTING,
                &self.previous_session_keys,
            ))
            .chain(Self::named(
                Self::SHELL_TOGGLE_SETTING,
                &self.shell_toggle_keys,
            ))
            .collect()
    }

    pub fn binds(&self, key: &HotKey) -> bool {
        self.all().any(|bound| bound == key)
    }

    fn named<'a>(
        setting: &'static str,
        keys: &'a [HotKey],
    ) -> impl Iterator<Item = (&'static str, &'a HotKey)> {
        keys.iter().map(move |key| (setting, key))
    }
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self::new(
            HotKey::parse(Self::RETURN_KEY).expect("the default return key parses"),
            HotKey::parse_valid(Self::NEXT_SESSION_KEYS),
            HotKey::parse_valid(Self::PREVIOUS_SESSION_KEYS),
        )
        .with_shell_toggle_keys(HotKey::parse_valid(Self::SHELL_TOGGLE_KEYS))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct BindingsFixture;

    impl BindingsFixture {
        fn names(keys: &[HotKey]) -> Vec<String> {
            keys.iter().map(HotKey::to_string).collect()
        }
    }

    #[test]
    fn the_defaults_are_ctrl_q_alt_j_and_alt_down_and_alt_k_and_alt_up() {
        let defaults = KeyBindings::default();

        assert_eq!(defaults.return_key().to_string(), "C-q");
        assert_eq!(
            BindingsFixture::names(defaults.next_session_keys()),
            ["M-j", "M-Down"]
        );
        assert_eq!(
            BindingsFixture::names(defaults.previous_session_keys()),
            ["M-k", "M-Up"]
        );
    }

    #[test]
    fn every_key_is_named_with_its_setting() {
        let defaults = KeyBindings::default();

        let settings: Vec<(&str, String)> = defaults
            .by_setting()
            .into_iter()
            .map(|(setting, key)| (setting, key.to_string()))
            .collect();

        assert_eq!(
            settings,
            [
                ("return_key", "C-q".to_string()),
                ("next_session_keys", "M-j".to_string()),
                ("next_session_keys", "M-Down".to_string()),
                ("previous_session_keys", "M-k".to_string()),
                ("previous_session_keys", "M-Up".to_string()),
                ("shell_toggle_keys", "M-l".to_string()),
            ]
        );
    }

    #[test]
    fn the_default_shell_toggle_key_is_alt_l() {
        let defaults = KeyBindings::default();

        assert_eq!(
            BindingsFixture::names(defaults.shell_toggle_keys()),
            ["M-l"]
        );
    }
}
