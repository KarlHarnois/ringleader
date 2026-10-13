use super::tmux_output::TmuxOutput;
use super::tmux_server_config::TmuxServerConfig;
use crate::domain::{HotKey, KeyBindings};

pub struct RunningConfig {
    hash: String,
    key_bindings: Option<KeyBindings>,
}

impl RunningConfig {
    const LABEL: &'static str = "ringleader-config";
    const NEXT_SESSION_LABEL: &'static str = "ringleader-next-session-keys";
    const PREVIOUS_SESSION_LABEL: &'static str = "ringleader-previous-session-keys";

    pub fn formats() -> [String; 3] {
        [
            format!(
                "{} #{{{}}} #{{{}}}",
                Self::LABEL,
                TmuxServerConfig::HASH_OPTION,
                TmuxServerConfig::RETURN_KEY_OPTION
            ),
            Self::labeled(
                Self::NEXT_SESSION_LABEL,
                TmuxServerConfig::NEXT_SESSION_KEYS_OPTION,
            ),
            Self::labeled(
                Self::PREVIOUS_SESSION_LABEL,
                TmuxServerConfig::PREVIOUS_SESSION_KEYS_OPTION,
            ),
        ]
    }

    pub fn read(output: &TmuxOutput) -> Option<Self> {
        let values = output.first(Self::LABEL)?;
        let (hash, return_key) = values.split_once(' ').unwrap_or((values, ""));
        let key_bindings = HotKey::parse(return_key).map(|return_key| {
            KeyBindings::new(
                return_key,
                Self::keys(output, Self::NEXT_SESSION_LABEL),
                Self::keys(output, Self::PREVIOUS_SESSION_LABEL),
            )
        });
        Some(Self {
            hash: hash.to_string(),
            key_bindings,
        })
    }

    pub fn hash(&self) -> &str {
        &self.hash
    }

    pub fn key_bindings(&self) -> Option<&KeyBindings> {
        self.key_bindings.as_ref()
    }

    fn labeled(label: &str, option: &str) -> String {
        format!("{label} #{{{option}}}")
    }

    fn keys(output: &TmuxOutput, label: &'static str) -> Vec<HotKey> {
        let listed = output.first(label).unwrap_or_default();
        HotKey::parse_valid(listed.split_whitespace())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RunningFixture;

    impl RunningFixture {
        fn read(stdout: &str) -> RunningConfig {
            RunningConfig::read(&TmuxOutput::new(true, stdout, "")).unwrap()
        }

        fn names(keys: &[HotKey]) -> Vec<String> {
            keys.iter().map(HotKey::to_string).collect()
        }
    }

    #[test]
    fn the_hash_and_all_three_key_settings_are_read() {
        let running = RunningFixture::read(concat!(
            "ringleader-config 0123456789abcdef M-x\n",
            "ringleader-next-session-keys M-n F7\n",
            "ringleader-previous-session-keys M-p\n",
        ));

        let bindings = running.key_bindings().unwrap();
        assert_eq!(running.hash(), "0123456789abcdef");
        assert_eq!(bindings.return_key().to_string(), "M-x");
        assert_eq!(
            RunningFixture::names(bindings.next_session()),
            ["M-n", "F7"]
        );
        assert_eq!(RunningFixture::names(bindings.previous_session()), ["M-p"]);
    }

    #[test]
    fn a_server_without_the_session_key_options_reads_as_empty_lists() {
        let running = RunningFixture::read(concat!(
            "ringleader-config 0123456789abcdef C-q\n",
            "ringleader-next-session-keys \n",
            "ringleader-previous-session-keys \n",
        ));

        let bindings = running.key_bindings().unwrap();
        assert!(bindings.next_session().is_empty());
        assert!(bindings.previous_session().is_empty());
    }
}
