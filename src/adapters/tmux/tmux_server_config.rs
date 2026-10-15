use std::slice;

use super::tmux_sequence::TmuxSequence;
use super::tmux_shell_toggle::TmuxShellToggle;
use crate::adapters::filesystem::fnv_hash::FnvHash;
use crate::domain::{HotKey, KeyBindings};

pub struct TmuxServerConfig {
    key_bindings: KeyBindings,
}

impl TmuxServerConfig {
    const RESET: &'static str = "set -su terminal-features";
    pub const COPY_TABLES: [&'static str; 2] = ["copy-mode", "copy-mode-vi"];
    const ROOT_TABLE: &'static str = "root";
    const BINDING_TABLES: [&'static str; 3] =
        [Self::ROOT_TABLE, Self::COPY_TABLES[0], Self::COPY_TABLES[1]];
    const SETTINGS: [&'static str; 16] = [
        "set -g prefix None",
        "set -g prefix2 None",
        "set -g status off",
        "set -g mouse on",
        "unbind-key -q -T root MouseDown3Pane",
        "unbind-key -q -T root M-MouseDown3Pane",
        "set -s escape-time 10",
        "set -g default-terminal tmux-256color",
        "set -g history-limit 25000",
        "set -s extended-keys always",
        "set -as terminal-features ',*:extkeys'",
        "set -as terminal-features ',*:RGB'",
        "set -s set-clipboard on",
        "set -s exit-unattached off",
        "set -g destroy-unattached off",
        "set -g detach-on-destroy on",
    ];
    const NEXT_SESSION: &'static str = "switch-client -n";
    const PREVIOUS_SESSION: &'static str = "switch-client -p";
    const RETURN: &'static str = "detach-client";
    const EMACS_COPY_MODE: &'static str = "copy-mode";
    const EMACS_COPY_MODE_DEFAULTS: [(&'static str, &'static str); 2] =
        [("M-Up", "halfpage-up"), ("M-Down", "halfpage-down")];

    pub const RETURN_KEY_OPTION: &'static str = "@ringleader_return_key";
    pub const NEXT_SESSION_KEYS_OPTION: &'static str = "@ringleader_next_session_keys";
    pub const PREVIOUS_SESSION_KEYS_OPTION: &'static str = "@ringleader_previous_session_keys";
    pub const SHELL_TOGGLE_KEYS_OPTION: &'static str = "@ringleader_shell_toggle_keys";
    pub const HASH_OPTION: &'static str = "@ringleader_config";

    pub fn new(key_bindings: &KeyBindings) -> Self {
        Self {
            key_bindings: key_bindings.clone(),
        }
    }

    pub fn reapply_script(previous: Option<&KeyBindings>, current: &KeyBindings) -> TmuxSequence {
        let dropped = previous
            .into_iter()
            .flat_map(KeyBindings::all)
            .filter(|key| !current.binds(key));
        let released = dropped.fold(TmuxSequence::new(), Self::released);
        released.command(["source-file", "-"])
    }

    pub fn render(&self) -> String {
        let hash_line = format!("set -g {} '{}'", Self::HASH_OPTION, self.hash());
        Self::joined(&[self.leading_lines(), vec![hash_line], self.bindings()].concat())
    }

    pub fn hash(&self) -> String {
        let hashed = [self.leading_lines(), self.bindings()].concat();
        FnvHash::of(Self::joined(&hashed).as_bytes()).hex()
    }

    fn leading_lines(&self) -> Vec<String> {
        let mut lines = vec![Self::RESET.to_string()];
        lines.extend(Self::SETTINGS.iter().map(|line| line.to_string()));
        lines.extend(self.recorded_keys());
        lines
    }

    fn recorded_keys(&self) -> [String; 4] {
        let bindings = &self.key_bindings;
        [
            Self::option_line(
                Self::RETURN_KEY_OPTION,
                &bindings.return_key().tmux_spelling(),
            ),
            Self::option_line(
                Self::NEXT_SESSION_KEYS_OPTION,
                &Self::key_list(bindings.next_session_keys()),
            ),
            Self::option_line(
                Self::PREVIOUS_SESSION_KEYS_OPTION,
                &Self::key_list(bindings.previous_session_keys()),
            ),
            Self::option_line(
                Self::SHELL_TOGGLE_KEYS_OPTION,
                &Self::key_list(bindings.shell_toggle_keys()),
            ),
        ]
    }

    fn option_line(option: &str, value: &str) -> String {
        format!("set -g {option} {value}")
    }

    fn key_list(keys: &[HotKey]) -> String {
        let names: Vec<String> = keys.iter().map(HotKey::to_string).collect();
        format!("'{}'", names.join(" "))
    }

    fn joined(lines: &[String]) -> String {
        lines.iter().map(|line| format!("{line}\n")).collect()
    }

    fn released(sequence: TmuxSequence, key: &HotKey) -> TmuxSequence {
        let name = key.to_string();
        let unbound = Self::BINDING_TABLES
            .iter()
            .fold(sequence, |sequence, table| {
                sequence.command(["unbind-key", "-q", "-T", table, &name])
            });
        Self::with_emacs_copy_mode_default(unbound, &name)
    }

    fn with_emacs_copy_mode_default(sequence: TmuxSequence, name: &str) -> TmuxSequence {
        let default = Self::EMACS_COPY_MODE_DEFAULTS
            .iter()
            .find(|(key, _)| *key == name);
        let Some((_, action)) = default else {
            return sequence;
        };
        sequence.command([
            "bind-key",
            "-T",
            Self::EMACS_COPY_MODE,
            name,
            "send-keys",
            "-X",
            action,
        ])
    }

    fn bindings(&self) -> Vec<String> {
        Self::BINDING_TABLES
            .iter()
            .flat_map(|table| self.table_bindings(table))
            .collect()
    }

    fn table_bindings(&self, table: &str) -> Vec<String> {
        let bindings = &self.key_bindings;
        let return_key = slice::from_ref(bindings.return_key());
        [
            Self::bound(table, bindings.next_session_keys(), Self::NEXT_SESSION),
            Self::bound(
                table,
                bindings.previous_session_keys(),
                Self::PREVIOUS_SESSION,
            ),
            Self::bound(
                table,
                bindings.shell_toggle_keys(),
                &TmuxShellToggle::command(),
            ),
            Self::bound(table, return_key, Self::RETURN),
        ]
        .concat()
    }

    fn bound(table: &str, keys: &[HotKey], command: &str) -> Vec<String> {
        keys.iter()
            .map(|key| format!("bind-key -T {table} {} {command}", key.tmux_spelling()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::ffi::OsStr;
    use std::fs;
    use std::path::Path;
    use std::time::Duration;

    use super::*;
    use crate::adapters::tmux::server_files::ServerFiles;
    use crate::adapters::tmux::tmux_command::TmuxCommand;
    use crate::adapters::tmux::tmux_sequence::TmuxSequence;
    use crate::application::ports::Multiplexer;
    use crate::test_support::TmuxTestServer;

    const GOLDEN: &str = include_str!("../../../tests/fixtures/tmux-server.conf");
    const KEY_CLASSES: [&str; 6] = ["C-q", "M-x", "F12", "~", "}", "M-}"];
    const DETACH_LIMIT: Duration = Duration::from_secs(2);

    struct ConfigFixture;

    impl ConfigFixture {
        fn render(key: &str) -> String {
            let bindings = KeyBindings::default().with_return_key(HotKey::parse(key).unwrap());
            TmuxServerConfig::new(&bindings).render()
        }

        fn binding(key: &str) -> String {
            format!(
                "bind-key -T root {} detach-client\n",
                HotKey::parse(key).unwrap().tmux_spelling()
            )
        }

        fn detaches_when_typed(key: &str, typed_bytes: &[&str]) -> bool {
            let inner = TmuxTestServer::with_config(&Self::render(key));
            inner.start_session("holder");
            Self::nested_client_detaches(&inner, "=holder:", typed_bytes)
        }

        fn nested_client_detaches(inner: &TmuxTestServer, target: &str, typed: &[&str]) -> bool {
            let outer = inner.nested_client(target);
            let attached = inner.wait_for(|| Self::clients(inner) == 1);
            outer.type_bytes(typed);
            attached && inner.wait_for(|| Self::clients(inner) == 0)
        }

        fn nested_client_on_beta_lands_on(typed: &[&str], expected: &str) -> bool {
            let inner = Self::alpha_beta_and_gamma();
            let outer = inner.nested_client("=beta:");
            let attached = inner.wait_for(|| Self::client_session(&inner) == "beta");
            outer.type_bytes(typed);
            attached && inner.wait_for(|| Self::client_session(&inner) == expected)
        }

        fn alpha_beta_and_gamma() -> TmuxTestServer {
            let server = TmuxTestServer::with_session("alpha");
            server.start_session("beta");
            server.start_session("gamma");
            server
        }

        fn session_after_pressing(server: &TmuxTestServer, target: &str, key: &str) -> String {
            let client = server.control_client(target);
            let before = Self::client_session(server);
            server.press(&client, key);
            server.wait_for(|| Self::client_session(server) != before);
            Self::client_session(server)
        }

        fn client_session(server: &TmuxTestServer) -> String {
            server.text(&["list-clients", "-F", "#{client_session}"])
        }

        fn clients(server: &TmuxTestServer) -> usize {
            server
                .text(&["list-clients", "-F", "client"])
                .lines()
                .count()
        }

        fn detach_keys(server: &TmuxTestServer) -> Vec<String> {
            server
                .text(&["list-keys", "-T", "root"])
                .lines()
                .filter(|line| line.ends_with(" detach-client"))
                .filter_map(|line| line.split_whitespace().nth(3))
                .map(|key| key.trim_matches('"').trim_start_matches('\\').to_string())
                .collect()
        }

        fn returns_from(server: &TmuxTestServer, target: &str) -> bool {
            let mut client = server.control_client(target);
            server.press(&client, "C-q");
            client.exits_with_notice_within(DETACH_LIMIT)
        }

        fn reapply_words(previous: &KeyBindings, current: &KeyBindings) -> Vec<String> {
            TmuxServerConfig::reapply_script(Some(previous), current)
                .argv()
                .iter()
                .map(|word| word.to_string_lossy().into_owned())
                .collect()
        }

        fn option(server: &TmuxTestServer, name: &str) -> String {
            server.text(&["show-options", "-g", "-q", "-v", name])
        }
    }

    #[test]
    fn the_default_render_matches_the_golden_fixture() {
        let render = TmuxServerConfig::new(&KeyBindings::default()).render();

        assert_eq!(render, GOLDEN);
    }

    #[test]
    fn the_reapply_unbinds_a_dropped_key_in_every_table_before_sourcing() {
        let changed =
            KeyBindings::default().with_next_session_keys(HotKey::parse_valid(["M-n", "M-Down"]));

        let words = ConfigFixture::reapply_words(&KeyBindings::default(), &changed);

        assert_eq!(
            words.join(" "),
            "unbind-key -q -T root M-j ; unbind-key -q -T copy-mode M-j ; \
             unbind-key -q -T copy-mode-vi M-j ; source-file -"
        );
    }

    #[test]
    fn the_reapply_never_unbinds_a_key_that_stays_bound_under_any_setting() {
        let swapped = KeyBindings::default()
            .with_next_session_keys(HotKey::parse_valid(["M-k", "M-Down"]))
            .with_previous_session_keys(HotKey::parse_valid(["M-j", "M-Up"]));

        let words = ConfigFixture::reapply_words(&KeyBindings::default(), &swapped);

        assert_eq!(words, ["source-file", "-"]);
    }

    #[test]
    fn a_server_started_with_the_render_has_no_prefix_and_no_status_line() {
        let server = TmuxTestServer::with_session("holder");

        let prefix = ConfigFixture::option(&server, "prefix");
        let status = ConfigFixture::option(&server, "status");

        assert_eq!((prefix.as_str(), status.as_str()), ("None", "off"));
    }

    #[test]
    fn the_first_pane_keeps_twenty_five_thousand_lines_of_history() {
        let server = TmuxTestServer::with_session("holder");

        let limit = server.text(&[
            "display-message",
            "-p",
            "-t",
            "=holder:",
            "#{history_limit}",
        ]);

        assert_eq!(limit, "25000");
    }

    #[test]
    fn sourcing_the_render_again_on_a_running_server_succeeds() {
        let server = TmuxTestServer::with_session("holder");

        let sourced = server.source(&ConfigFixture::render(KeyBindings::RETURN_KEY));

        assert!(sourced.status.success(), "{sourced:?}");
    }

    #[test]
    fn every_key_class_binds_detach_client_at_server_start() {
        for key in KEY_CLASSES {
            let server = TmuxTestServer::with_config(&ConfigFixture::render(key));
            server.start_session("holder");

            assert_eq!(ConfigFixture::detach_keys(&server), [key], "{key}");
        }
    }

    #[test]
    fn every_key_class_binds_detach_client_through_source_file() {
        let server = TmuxTestServer::with_config("");
        server.start_session("holder");

        for key in KEY_CLASSES {
            let sourced = server.source(&ConfigFixture::binding(key));

            assert!(sourced.status.success(), "{key}: {sourced:?}");
        }
        let mut bound = ConfigFixture::detach_keys(&server);
        bound.sort();
        let mut expected = KEY_CLASSES.map(str::to_string);
        expected.sort();
        assert_eq!(bound, expected);
    }

    #[test]
    fn every_printable_character_the_return_key_accepts_binds_with_each_modifier() {
        let server = TmuxTestServer::with_config("");
        server.start_session("holder");
        let accepted: BTreeSet<String> = (b'!'..=b'~')
            .flat_map(|byte| {
                ["", "C-", "M-"].map(|modifier| format!("{modifier}{}", char::from(byte)))
            })
            .filter_map(|key| HotKey::parse(&key))
            .map(|key| key.to_string())
            .collect();

        let bindings: String = accepted
            .iter()
            .map(|key| ConfigFixture::binding(key))
            .collect();
        let sourced = server.source(&bindings);

        assert!(sourced.status.success(), "{sourced:?}");
        assert_eq!(ConfigFixture::detach_keys(&server).len(), accepted.len());
    }

    #[test]
    fn the_bytes_a_terminal_sends_for_each_key_class_detach_a_nested_client() {
        for (key, typed) in [
            ("C-q", ["11"].as_slice()),
            ("C-Q", &["11"]),
            ("C-]", &["1d"]),
            ("M-x", &["1b", "78"]),
        ] {
            assert!(ConfigFixture::detaches_when_typed(key, typed), "{key}");
        }
    }

    #[test]
    fn the_return_key_detaches_a_pane_in_either_copy_mode() {
        for mode_keys in ["emacs", "vi"] {
            let inner = TmuxTestServer::with_config(&ConfigFixture::render("M-x"));
            inner.start_session("holder");
            inner.text(&["set-option", "-g", "mode-keys", mode_keys]);
            inner.text(&["copy-mode", "-t", "=holder:"]);

            let detached = ConfigFixture::nested_client_detaches(&inner, "=holder:", &["1b", "78"]);

            assert!(detached, "{mode_keys}");
        }
    }

    #[test]
    fn bytes_for_another_key_leave_a_nested_client_attached() {
        assert!(!ConfigFixture::detaches_when_typed("M-x", &["11"]));
    }

    #[test]
    fn the_bytes_for_ctrl_h_and_ctrl_j_detach_a_nested_client() {
        for (key, typed) in [("C-h", ["08"]), ("C-j", ["0a"])] {
            assert!(ConfigFixture::detaches_when_typed(key, &typed), "{key}");
        }
    }

    #[test]
    fn the_return_key_detaches_a_client_from_an_agent_window() {
        let server = TmuxTestServer::new();
        server.managed_session("fix-auth");

        assert!(ConfigFixture::returns_from(
            &server,
            "=ringleader-fix-auth:"
        ));
    }

    #[test]
    fn the_return_key_detaches_a_client_from_a_shell_window() {
        let server = TmuxTestServer::new();
        let worktree = server.managed_session("fix-auth");
        let window = server
            .multiplexer()
            .ensure_shell_window("ringleader-fix-auth", &worktree, "sh")
            .unwrap();

        assert!(ConfigFixture::returns_from(&server, &window));
    }

    #[test]
    fn the_return_key_detaches_a_client_from_a_session_without_metadata() {
        let server = TmuxTestServer::with_session("work");

        assert!(ConfigFixture::returns_from(&server, "=work:"));
    }

    #[test]
    fn the_next_session_key_moves_the_client_to_the_next_session() {
        let server = ConfigFixture::alpha_beta_and_gamma();

        let session = ConfigFixture::session_after_pressing(&server, "=beta:", "M-j");

        assert_eq!(session, "gamma");
    }

    #[test]
    fn the_previous_session_key_moves_the_client_back() {
        let server = ConfigFixture::alpha_beta_and_gamma();

        let session = ConfigFixture::session_after_pressing(&server, "=beta:", "M-k");

        assert_eq!(session, "alpha");
    }

    #[test]
    fn the_next_session_key_on_the_last_session_wraps_to_the_first() {
        let server = ConfigFixture::alpha_beta_and_gamma();

        let session = ConfigFixture::session_after_pressing(&server, "=gamma:", "M-Down");

        assert_eq!(session, "alpha");
    }

    #[test]
    fn the_previous_session_key_on_the_first_session_wraps_to_the_last() {
        let server = ConfigFixture::alpha_beta_and_gamma();

        let session = ConfigFixture::session_after_pressing(&server, "=alpha:", "M-Up");

        assert_eq!(session, "gamma");
    }

    #[test]
    fn the_session_keys_switch_from_either_copy_mode() {
        for (mode_keys, key) in [
            ("emacs", "M-j"),
            ("emacs", "M-Down"),
            ("vi", "M-j"),
            ("vi", "M-Down"),
        ] {
            let server = ConfigFixture::alpha_beta_and_gamma();
            server.text(&["set-option", "-g", "mode-keys", mode_keys]);
            server.text(&["copy-mode", "-t", "=beta:"]);

            let session = ConfigFixture::session_after_pressing(&server, "=beta:", key);

            assert_eq!(session, "gamma", "{mode_keys} {key}");
        }
    }

    #[test]
    fn the_session_keys_switch_from_a_shell_window() {
        let server = TmuxTestServer::new();
        let worktree = server.managed_session("fix-auth");
        server.managed_session("other");
        let window = server
            .multiplexer()
            .ensure_shell_window("ringleader-fix-auth", &worktree, "sh")
            .unwrap();

        let session = ConfigFixture::session_after_pressing(&server, &window, "M-j");

        assert_eq!(session, "ringleader-other");
    }

    #[test]
    fn the_bytes_a_terminal_sends_for_the_session_keys_switch_a_nested_client() {
        for (key, typed, expected) in [
            ("M-j", ["1b", "6a"].as_slice(), "gamma"),
            ("M-Down", &["1b", "5b", "31", "3b", "33", "42"], "gamma"),
            ("M-k", &["1b", "6b"], "alpha"),
            ("M-Up", &["1b", "5b", "31", "3b", "33", "41"], "alpha"),
        ] {
            assert!(
                ConfigFixture::nested_client_on_beta_lands_on(typed, expected),
                "{key}"
            );
        }
    }

    #[test]
    fn ctrl_b_reaches_the_pane() {
        let server = TmuxTestServer::new();
        server.text(&["new-session", "-d", "-s", "work", "cat", "-v"]);
        let client = server.control_client("=work:");

        server.press(&client, "C-b");
        server.press(&client, "Enter");

        assert!(server.wait_for(|| {
            server
                .text(&["capture-pane", "-p", "-t", "=work:"])
                .contains("^B")
        }));
    }

    #[test]
    fn a_config_path_with_brackets_loads() {
        let server = TmuxTestServer::new();
        let config = server.directory().join("conf[1]").join("tmux.conf");
        let files = ServerFiles::new(server.socket(), &config);
        files
            .prepare(&ConfigFixture::render(KeyBindings::RETURN_KEY))
            .unwrap();
        let tmux = TmuxCommand::new(OsStr::new("tmux"), &files, &server.environment());

        tmux.create(
            &TmuxSequence::new().command(["new-session", "-d", "-s", "holder", "sleep", "30"]),
            &[],
        )
        .unwrap();

        assert_eq!(ConfigFixture::option(&server, "prefix"), "None");
    }

    #[test]
    fn the_users_tmux_config_files_never_load() {
        let server = TmuxTestServer::new();
        let home = server.directory();
        fs::write(home.join(".tmux.conf"), "set -g @example-marker home\n").unwrap();
        let xdg = Path::new(home).join(".config").join("tmux");
        fs::create_dir_all(&xdg).unwrap();
        fs::write(xdg.join("tmux.conf"), "set -g @example-marker xdg\n").unwrap();

        server.managed_session("fix-auth");

        assert_eq!(ConfigFixture::option(&server, "@example-marker"), "");
    }
}
