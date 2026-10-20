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
    const SHELL_TOGGLE_LABEL: &'static str = "ringleader-shell-toggle-keys";
    const FINDER_LABEL: &'static str = "ringleader-finder-keys";

    pub fn formats() -> [String; 5] {
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
            Self::labeled(
                Self::SHELL_TOGGLE_LABEL,
                TmuxServerConfig::SHELL_TOGGLE_KEYS_OPTION,
            ),
            Self::labeled(Self::FINDER_LABEL, TmuxServerConfig::FINDER_KEYS_OPTION),
        ]
    }

    pub fn read(output: &TmuxOutput) -> Option<Self> {
        let values = output.first(Self::LABEL)?;
        let (hash, return_key) = values.split_once(' ').unwrap_or((values, ""));
        let key_bindings = HotKey::recorded(return_key).map(|return_key| {
            KeyBindings::new(
                return_key,
                Self::keys(output, Self::NEXT_SESSION_LABEL),
                Self::keys(output, Self::PREVIOUS_SESSION_LABEL),
                Self::keys(output, Self::SHELL_TOGGLE_LABEL),
                Self::keys(output, Self::FINDER_LABEL),
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
        listed
            .split_whitespace()
            .filter_map(HotKey::recorded)
            .collect()
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
    fn the_hash_and_all_five_key_settings_are_read() {
        let running = RunningFixture::read(concat!(
            "ringleader-config 0123456789abcdef M-x\n",
            "ringleader-next-session-keys M-n F7\n",
            "ringleader-previous-session-keys M-p\n",
            "ringleader-shell-toggle-keys M-o F8\n",
            "ringleader-finder-keys M-f\n",
        ));

        let bindings = running.key_bindings().unwrap();
        assert_eq!(running.hash(), "0123456789abcdef");
        assert_eq!(bindings.return_key().to_string(), "M-x");
        assert_eq!(
            RunningFixture::names(bindings.next_session_keys()),
            ["M-n", "F7"]
        );
        assert_eq!(
            RunningFixture::names(bindings.previous_session_keys()),
            ["M-p"]
        );
        assert_eq!(
            RunningFixture::names(bindings.shell_toggle_keys()),
            ["M-o", "F8"]
        );
        assert_eq!(RunningFixture::names(bindings.finder_keys()), ["M-f"]);
    }

    #[test]
    fn a_server_without_the_shell_toggle_option_reads_as_no_toggle_keys() {
        let running = RunningFixture::read(concat!(
            "ringleader-config 0123456789abcdef C-q\n",
            "ringleader-next-session-keys M-j M-Down\n",
            "ringleader-previous-session-keys M-k M-Up\n",
            "ringleader-shell-toggle-keys \n",
        ));

        let bindings = running.key_bindings().unwrap();
        assert!(bindings.shell_toggle_keys().is_empty());
    }

    #[test]
    fn a_server_without_the_finder_option_reads_as_no_finder_keys() {
        let running = RunningFixture::read(concat!(
            "ringleader-config 0123456789abcdef C-q\n",
            "ringleader-shell-toggle-keys M-l\n",
            "ringleader-finder-keys \n",
        ));

        let bindings = running.key_bindings().unwrap();
        assert!(bindings.finder_keys().is_empty());
    }

    #[test]
    fn keys_a_config_can_no_longer_set_are_still_read_off_the_server() {
        let running = RunningFixture::read(concat!(
            "ringleader-config 0123456789abcdef M-BSpace\n",
            "ringleader-next-session-keys M-BTab M-j\n",
            "ringleader-previous-session-keys M-k\n",
        ));

        let bindings = running.key_bindings().unwrap();
        assert_eq!(bindings.return_key().to_string(), "M-BSpace");
        assert_eq!(
            RunningFixture::names(bindings.next_session_keys()),
            ["M-BTab", "M-j"]
        );
    }

    #[test]
    fn a_server_without_the_session_key_options_reads_as_empty_lists() {
        let running = RunningFixture::read(concat!(
            "ringleader-config 0123456789abcdef C-q\n",
            "ringleader-next-session-keys \n",
            "ringleader-previous-session-keys \n",
        ));

        let bindings = running.key_bindings().unwrap();
        assert!(bindings.next_session_keys().is_empty());
        assert!(bindings.previous_session_keys().is_empty());
    }
}
