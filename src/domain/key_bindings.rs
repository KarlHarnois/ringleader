use std::iter::once;

use super::hot_key::HotKey;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBindings {
    return_key: HotKey,
    next_session: Vec<HotKey>,
    previous_session: Vec<HotKey>,
}

impl KeyBindings {
    pub const RETURN_KEY: &'static str = "C-q";
    pub const NEXT_SESSION_KEYS: [&'static str; 2] = ["M-j", "M-Down"];
    pub const PREVIOUS_SESSION_KEYS: [&'static str; 2] = ["M-k", "M-Up"];
    pub const RETURN_KEY_SETTING: &'static str = "return_key";
    pub const NEXT_SESSION_SETTING: &'static str = "next_session_keys";
    pub const PREVIOUS_SESSION_SETTING: &'static str = "previous_session_keys";

    pub fn new(
        return_key: HotKey,
        next_session: Vec<HotKey>,
        previous_session: Vec<HotKey>,
    ) -> Self {
        Self {
            return_key,
            next_session,
            previous_session,
        }
    }

    #[cfg(test)]
    pub fn with_return_key(self, return_key: HotKey) -> Self {
        Self { return_key, ..self }
    }

    #[cfg(test)]
    pub fn with_next_session(self, next_session: Vec<HotKey>) -> Self {
        Self {
            next_session,
            ..self
        }
    }

    #[cfg(test)]
    pub fn with_previous_session(self, previous_session: Vec<HotKey>) -> Self {
        Self {
            previous_session,
            ..self
        }
    }

    pub fn return_key(&self) -> &HotKey {
        &self.return_key
    }

    pub fn next_session(&self) -> &[HotKey] {
        &self.next_session
    }

    pub fn previous_session(&self) -> &[HotKey] {
        &self.previous_session
    }

    pub fn all(&self) -> impl Iterator<Item = &HotKey> {
        self.by_setting().into_iter().map(|(_, key)| key)
    }

    pub fn by_setting(&self) -> Vec<(&'static str, &HotKey)> {
        let next = self
            .next_session
            .iter()
            .map(|key| (Self::NEXT_SESSION_SETTING, key));
        let previous = self
            .previous_session
            .iter()
            .map(|key| (Self::PREVIOUS_SESSION_SETTING, key));
        once((Self::RETURN_KEY_SETTING, &self.return_key))
            .chain(next)
            .chain(previous)
            .collect()
    }

    pub fn binds(&self, key: &HotKey) -> bool {
        self.all().any(|bound| bound == key)
    }
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self::new(
            HotKey::parse(Self::RETURN_KEY).expect("the default return key parses"),
            HotKey::parse_valid(Self::NEXT_SESSION_KEYS),
            HotKey::parse_valid(Self::PREVIOUS_SESSION_KEYS),
        )
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
            BindingsFixture::names(defaults.next_session()),
            ["M-j", "M-Down"]
        );
        assert_eq!(
            BindingsFixture::names(defaults.previous_session()),
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
            ]
        );
    }
}
