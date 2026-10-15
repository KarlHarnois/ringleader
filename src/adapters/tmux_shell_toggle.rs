use super::tmux_listing::TmuxListing;

pub struct TmuxShellToggle;

impl TmuxShellToggle {
    const WORKTREE: &'static str = "#{session_path}";

    pub fn command() -> String {
        let toggled = Self::either(
            &Self::on_shell_window(),
            &Self::shown(&Self::agent_window()),
            &Self::shell_window_shown(),
        );
        Self::only_if(&Self::in_managed_session(), &toggled)
    }

    fn in_managed_session() -> String {
        Self::expanded(TmuxListing::METADATA_OPTION)
    }

    fn agent_window() -> String {
        let agent_pane_window = Self::window_id_if(TmuxListing::AGENT_TAG);
        Self::over_windows(&Self::over_panes(&agent_pane_window))
    }

    fn shell_window_shown() -> String {
        Self::either(
            &Self::has_shell_window(),
            &Self::shown(&Self::shell_window()),
            &Self::shell_window_created(),
        )
    }

    fn shell_window_created() -> String {
        format!(
            "new-window -c '{}' ; set-option -w {} 1",
            Self::WORKTREE,
            TmuxListing::SHELL_TAG
        )
    }

    fn on_shell_window() -> String {
        Self::expanded(TmuxListing::SHELL_TAG)
    }

    fn has_shell_window() -> String {
        Self::over_windows(&Self::on_shell_window())
    }

    fn shell_window() -> String {
        Self::over_windows(&Self::window_id_if(TmuxListing::SHELL_TAG))
    }

    fn shown(window: &str) -> String {
        format!("run-shell -C \"select-window -t '{window}'\"")
    }

    fn either(condition: &str, then: &str, otherwise: &str) -> String {
        format!("{} {{ {otherwise} }}", Self::only_if(condition, then))
    }

    fn only_if(condition: &str, then: &str) -> String {
        format!("if-shell -F '{condition}' {{ {then} }}")
    }

    fn over_windows(format: &str) -> String {
        Self::expanded(&format!("W:{format}"))
    }

    fn over_panes(format: &str) -> String {
        Self::expanded(&format!("P:{format}"))
    }

    fn window_id_if(tag: &str) -> String {
        Self::expanded(&format!("?{tag},#{{window_id}},"))
    }

    fn expanded(expression: &str) -> String {
        format!("#{{{expression}}}")
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use crate::adapters::tmux_multiplexer::TmuxMultiplexer;
    use crate::application::ports::Multiplexer;
    use crate::domain::KeyBindings;
    use crate::test_support::tmux_test_server::ControlClient;
    use crate::test_support::{ExecutableScript, TmuxTestServer};

    struct ToggleFixture;

    impl ToggleFixture {
        const TOGGLE_KEY: &'static str = "M-l";
        const TOGGLE_KEY_BYTES: [&'static str; 2] = ["1b", "6c"];
        const SESSION: &'static str = "=ringleader-fix-auth:";
        const TMUX_NAME: &'static str = "ringleader-fix-auth";
        const SHELL_SCRIPT: &'static str = "#!/bin/sh\nexec sleep 30\n";

        fn toggled(server: &TmuxTestServer, client: &ControlClient) -> String {
            Self::toggle_switches_window(server, client);
            Self::client_window(server)
        }

        fn client_window(server: &TmuxTestServer) -> String {
            server.text(&["list-clients", "-F", "#{window_id}"])
        }

        fn windows(server: &TmuxTestServer, session: &str) -> usize {
            server
                .text(&["list-windows", "-t", session, "-F", "#{window_id}"])
                .lines()
                .count()
        }

        fn value(server: &TmuxTestServer, target: &str, format: &str) -> String {
            server.text(&["display-message", "-p", "-t", target, format])
        }

        fn shell_window(server: &TmuxTestServer, worktree: &Path) -> String {
            server
                .multiplexer()
                .ensure_shell_window(Self::TMUX_NAME, worktree, &server.environment().shell())
                .unwrap()
        }

        fn agent_pane(server: &TmuxTestServer) -> String {
            let format = "#{?@ringleader_agent,#{pane_id},}";
            let panes = server.text(&["list-panes", "-s", "-t", Self::SESSION, "-F", format]);
            panes.lines().collect()
        }

        fn agent_window(server: &TmuxTestServer) -> String {
            Self::value(server, &Self::agent_pane(server), "#{window_id}")
        }

        fn starts_in(server: &TmuxTestServer, window: &str, worktree: &Path) -> bool {
            let expected = fs::canonicalize(worktree).unwrap().display().to_string();
            server.wait_for(|| Self::value(server, window, "#{pane_current_path}") == expected)
        }

        fn shell_named_with_a_quote(server: &TmuxTestServer) -> PathBuf {
            Self::shell_named(server, "it's a shell")
        }

        fn shell_named(server: &TmuxTestServer, name: &str) -> PathBuf {
            let shell = server.directory().join(name);
            ExecutableScript::write(&shell, Self::SHELL_SCRIPT);
            shell
        }

        fn start_command(server: &TmuxTestServer, window: &str) -> String {
            Self::value(server, window, "#{pane_start_command}")
        }

        fn runs_the_default_shell(server: &TmuxTestServer, window: &str) -> bool {
            Self::start_command(server, window).is_empty()
        }

        fn managed_session_without_a_recorded_shell(server: &TmuxTestServer) {
            server.managed_session("fix-auth");
            server.text(&["set-option", "-u", "-t", Self::SESSION, "default-command"]);
        }

        fn set_fixture_default_shell(server: &TmuxTestServer) {
            let shell = Self::shell_named(server, "default-shell");
            server.text(&[
                "set-option",
                "-g",
                "default-shell",
                &shell.display().to_string(),
            ]);
        }

        fn multiplexer_with_shell(server: &TmuxTestServer, shell: &Path) -> TmuxMultiplexer {
            let shell = shell.display().to_string();
            let environment = server.environment_with(&[("SHELL", &shell)]);
            TmuxMultiplexer::new(server.command_with(&environment), server.stand_in())
        }

        fn toggles_from_copy_mode(
            server: &TmuxTestServer,
            client: &ControlClient,
            shown: &str,
        ) -> bool {
            server.text(&["copy-mode", "-t", shown]);
            Self::toggle_switches_window(server, client)
        }

        fn toggle_switches_window(server: &TmuxTestServer, client: &ControlClient) -> bool {
            let before = Self::client_window(server);
            server.press(client, Self::TOGGLE_KEY);
            server.wait_for(|| Self::client_window(server) != before)
        }
    }

    #[test]
    fn from_the_agent_window_the_toggle_key_shows_a_new_shell_window() {
        let server = TmuxTestServer::new();
        server.managed_session("fix-auth");
        let client = server.control_client(ToggleFixture::SESSION);

        let shown = ToggleFixture::toggled(&server, &client);

        assert_ne!(shown, ToggleFixture::agent_window(&server));
        assert_eq!(ToggleFixture::windows(&server, ToggleFixture::SESSION), 2);
    }

    #[test]
    fn the_shell_window_the_toggle_key_opens_starts_in_the_worktree() {
        let server = TmuxTestServer::new();
        let worktree = server.managed_session("fix-auth");
        let client = server.control_client(ToggleFixture::SESSION);

        let shown = ToggleFixture::toggled(&server, &client);

        assert!(ToggleFixture::starts_in(&server, &shown, &worktree));
    }

    #[test]
    fn a_shell_path_with_a_quote_starts_alike_from_the_toggle_key_and_t() {
        let server = TmuxTestServer::new();
        let shell = ToggleFixture::shell_named_with_a_quote(&server);
        let multiplexer = ToggleFixture::multiplexer_with_shell(&server, &shell);
        server.managed_session_through(&multiplexer, "fix-auth");
        let other = server.managed_session_through(&multiplexer, "other");
        let client = server.control_client(ToggleFixture::SESSION);

        let toggled = ToggleFixture::toggled(&server, &client);
        let opened_by_t = multiplexer
            .ensure_shell_window("ringleader-other", &other, &shell.display().to_string())
            .unwrap();

        assert_eq!(
            ToggleFixture::start_command(&server, &toggled),
            ToggleFixture::start_command(&server, &opened_by_t)
        );
    }

    #[test]
    fn the_toggle_key_runs_the_shell_of_the_process_that_created_the_session() {
        let server = TmuxTestServer::new();
        let creators_shell = ToggleFixture::shell_named(&server, "creators-shell");
        let creator = ToggleFixture::multiplexer_with_shell(&server, &creators_shell);
        server.managed_session_through(&creator, "fix-auth");
        let other_shell = ToggleFixture::shell_named(&server, "other-shell");
        ToggleFixture::multiplexer_with_shell(&server, &other_shell)
            .apply_config(&KeyBindings::default())
            .unwrap();
        let client = server.control_client(ToggleFixture::SESSION);

        let toggled = ToggleFixture::toggled(&server, &client);

        assert_eq!(
            ToggleFixture::start_command(&server, &toggled),
            creators_shell.display().to_string()
        );
    }

    #[test]
    fn in_a_session_with_no_recorded_shell_the_toggle_key_runs_the_default_shell() {
        let server = TmuxTestServer::new();
        ToggleFixture::managed_session_without_a_recorded_shell(&server);
        ToggleFixture::set_fixture_default_shell(&server);
        let client = server.control_client(ToggleFixture::SESSION);

        let toggled = ToggleFixture::toggled(&server, &client);

        assert!(ToggleFixture::runs_the_default_shell(&server, &toggled));
    }

    #[test]
    fn from_the_agent_window_the_toggle_key_shows_the_shell_window_t_opened() {
        let server = TmuxTestServer::new();
        let worktree = server.managed_session("fix-auth");
        let opened_by_t = ToggleFixture::shell_window(&server, &worktree);
        let client = server.control_client(&ToggleFixture::agent_pane(&server));

        let shown = ToggleFixture::toggled(&server, &client);

        assert_eq!(shown, opened_by_t);
        assert_eq!(ToggleFixture::windows(&server, ToggleFixture::SESSION), 2);
    }

    #[test]
    fn from_the_shell_window_the_toggle_key_shows_the_agent_window() {
        let server = TmuxTestServer::new();
        let worktree = server.managed_session("fix-auth");
        let shell = ToggleFixture::shell_window(&server, &worktree);
        let client = server.control_client(&shell);

        let shown = ToggleFixture::toggled(&server, &client);

        assert_eq!(shown, ToggleFixture::agent_window(&server));
    }

    #[test]
    fn t_attaches_to_the_shell_window_the_toggle_key_opened() {
        let server = TmuxTestServer::new();
        let worktree = server.managed_session("fix-auth");
        let client = server.control_client(ToggleFixture::SESSION);
        let toggled = ToggleFixture::toggled(&server, &client);

        let opened_by_t = ToggleFixture::shell_window(&server, &worktree);

        assert_eq!(opened_by_t, toggled);
    }

    #[test]
    fn the_toggle_key_opens_nothing_in_a_session_without_metadata() {
        let server = TmuxTestServer::with_session("work");
        let client = server.control_client("=work:");
        let window = ToggleFixture::client_window(&server);

        server.press(&client, ToggleFixture::TOGGLE_KEY);

        assert!(!server.wait_for(|| ToggleFixture::windows(&server, "=work:") > 1));
        assert_eq!(ToggleFixture::client_window(&server), window);
    }

    #[test]
    fn the_toggle_key_switches_from_either_copy_mode_on_either_window() {
        for mode_keys in ["emacs", "vi"] {
            let server = TmuxTestServer::new();
            let worktree = server.managed_session("fix-auth");
            let shell = ToggleFixture::shell_window(&server, &worktree);
            let agent = ToggleFixture::agent_pane(&server);
            server.text(&["set-option", "-g", "mode-keys", mode_keys]);
            let client = server.control_client(&agent);

            for shown in [&agent, &shell] {
                assert!(
                    ToggleFixture::toggles_from_copy_mode(&server, &client, shown),
                    "{mode_keys} {shown}"
                );
            }
        }
    }

    #[test]
    fn the_bytes_a_terminal_sends_for_the_toggle_key_toggle_a_nested_client() {
        let inner = TmuxTestServer::new();
        inner.managed_session("fix-auth");
        let agent_window = ToggleFixture::agent_window(&inner);
        let outer = inner.nested_client(ToggleFixture::SESSION);
        let attached = inner.wait_for(|| ToggleFixture::client_window(&inner) == agent_window);

        outer.type_bytes(&ToggleFixture::TOGGLE_KEY_BYTES);
        let on_shell = inner.wait_for(|| ToggleFixture::client_window(&inner) != agent_window);
        outer.type_bytes(&ToggleFixture::TOGGLE_KEY_BYTES);
        let back_on_agent = inner.wait_for(|| ToggleFixture::client_window(&inner) == agent_window);

        assert!(attached && on_shell && back_on_agent);
    }
}
