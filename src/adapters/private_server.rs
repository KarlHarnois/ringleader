use std::fs;
use std::io;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use super::key_switch::KeySwitch;
use super::live_process::LiveProcess;
use super::running_config::RunningConfig;
use super::tmux_command::TmuxCommand;
use super::tmux_error::TmuxError;
use super::tmux_launch::{Launched, TmuxLaunch};
use super::tmux_listing::TmuxListing;
use super::tmux_scripts::TmuxScripts;
use super::tmux_server_config::TmuxServerConfig;
use crate::application::ports::{MultiplexerError, ServerConfig, ServerState};
use crate::domain::KeyBindings;

pub struct PrivateServer<'a> {
    tmux: &'a TmuxCommand,
}

impl<'a> PrivateServer<'a> {
    const RESTORE_LIMIT: Duration = Duration::from_secs(2);
    const RESTORE_POLL: Duration = Duration::from_millis(20);

    pub fn new(tmux: &'a TmuxCommand) -> Self {
        Self { tmux }
    }

    pub fn write_files(&self, key_bindings: &KeyBindings) -> Result<(), MultiplexerError> {
        self.write_render(&self.render_for(key_bindings))
    }

    pub fn require_return_binding(&self, launched: &Launched) -> Result<(), MultiplexerError> {
        let bound = self.binds_return_key();
        if let Ok(true) = bound {
            return Ok(());
        }
        TmuxLaunch::new(self.tmux).roll_back(launched);
        bound?;
        let files = self.tmux.server();
        Err(MultiplexerError::NoReturnBinding {
            socket: files.socket().display().to_string(),
            config: files.config().display().to_string(),
        })
    }

    pub fn socket_lost(&self) -> bool {
        !self.servers_without_socket().is_empty()
    }

    pub fn restore_socket(&self) -> Result<bool, MultiplexerError> {
        let servers = self.servers_without_socket();
        if servers.is_empty() {
            return Ok(false);
        }
        self.tmux.server().prepare_socket_directory()?;
        let failures: Vec<String> = servers
            .iter()
            .filter_map(|server| {
                let error = server.signal(libc::SIGUSR1).err()?;
                Some(format!(
                    "SIGUSR1 to tmux server {} failed: {error}",
                    server.pid()
                ))
            })
            .collect();
        if failures.len() == servers.len() {
            return Err(self.socket_not_restored(&failures.join("; ")));
        }
        match self.socket_returns() {
            true => Ok(true),
            false => Err(self.socket_not_restored(&format!(
                "the server did not recreate it within {} seconds",
                Self::RESTORE_LIMIT.as_secs()
            ))),
        }
    }

    fn servers_without_socket(&self) -> Vec<LiveProcess> {
        let files = self.tmux.server();
        match Self::is_missing(files.socket()) && !files.server_is_starting() {
            true => LiveProcess::tmux_servers_on(files.socket()),
            false => Vec::new(),
        }
    }

    fn socket_returns(&self) -> bool {
        let deadline = Instant::now() + Self::RESTORE_LIMIT;
        while Self::is_missing(self.tmux.server().socket()) {
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Self::RESTORE_POLL);
        }
        true
    }

    fn is_missing(path: &Path) -> bool {
        fs::symlink_metadata(path).is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
    }

    fn socket_not_restored(&self, detail: &str) -> MultiplexerError {
        MultiplexerError::SocketNotRestored {
            socket: self.tmux.server().socket().display().to_string(),
            detail: detail.to_string(),
        }
    }

    pub fn state(&self) -> Result<Option<ServerState>, MultiplexerError> {
        let output = self.tmux.query(&TmuxScripts::server_state())?;
        if let Some(state) = TmuxListing::new(&output).server_state() {
            return Ok(Some(state));
        }
        if output.reports_no_server() {
            return Ok(None);
        }
        Err(TmuxError::failed("display-message", &output).into())
    }

    pub fn config(
        &self,
        key_bindings: &KeyBindings,
    ) -> Result<Option<ServerConfig>, MultiplexerError> {
        let Some(running) = self.running_config()? else {
            return Ok(None);
        };
        let current = running.hash() == self.render_for(key_bindings).hash();
        Ok(Some(ServerConfig::new(current, running.key_bindings())))
    }

    pub fn apply(&self, key_bindings: &KeyBindings) -> Result<(), MultiplexerError> {
        let running = self.running_config()?;
        let previous = running.as_ref().and_then(RunningConfig::key_bindings);
        let switch = KeySwitch::read(self.tmux, previous, key_bindings)?;
        let render = self.render_for(key_bindings);
        self.write_render(&render)?;
        let sequence = switch
            .saving()
            .then(TmuxServerConfig::reapply_script(previous, key_bindings));
        let input = format!("{}{}", switch.restoring_lines(), render.render());
        let output = self.tmux.change_with_input(&sequence, input.as_bytes())?;
        if output.succeeded() {
            return Ok(());
        }
        Err(TmuxError::failed("source-file", &output).into())
    }

    fn render_for(&self, key_bindings: &KeyBindings) -> TmuxServerConfig {
        TmuxServerConfig::new(key_bindings)
    }

    fn write_render(&self, render: &TmuxServerConfig) -> Result<(), MultiplexerError> {
        self.tmux.server().prepare(&render.render())
    }

    fn binds_return_key(&self) -> Result<bool, MultiplexerError> {
        let output = self.tmux.query(&TmuxScripts::root_keys())?;
        Ok(TmuxListing::new(&output).binds_detach())
    }

    fn running_config(&self) -> Result<Option<RunningConfig>, MultiplexerError> {
        let query = TmuxScripts::server_values(&RunningConfig::formats());
        let output = self.tmux.query(&query)?;
        if let Some(running) = RunningConfig::read(&output) {
            return Ok(Some(running));
        }
        if output.reports_no_server() {
            return Ok(None);
        }
        Err(TmuxError::failed("display-message", &output).into())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::domain::HotKey;
    use crate::test_support::TmuxTestServer;

    struct ServerFixture;

    impl ServerFixture {
        const TABLES: [&'static str; 3] = ["root", "copy-mode", "copy-mode-vi"];

        fn alt_x() -> KeyBindings {
            KeyBindings::default().with_return_key(HotKey::parse("M-x").unwrap())
        }

        fn alt_n_and_alt_down() -> KeyBindings {
            KeyBindings::default().with_next_session_keys(HotKey::parse_valid(["M-n", "M-Down"]))
        }

        fn start_lock(server: &TmuxTestServer) -> fs::File {
            let mut path = server.socket().as_os_str().to_os_string();
            path.push(".lock");
            fs::File::create(path).unwrap()
        }

        fn detach_keys(server: &TmuxTestServer) -> Vec<String> {
            Self::detach_keys_in(server, "root")
        }

        fn detach_keys_in(server: &TmuxTestServer, table: &str) -> Vec<String> {
            Self::keys_running(server, table, "detach-client")
        }

        fn keys_running(server: &TmuxTestServer, table: &str, command: &str) -> Vec<String> {
            let ending = format!(" {command}");
            let mut keys: Vec<String> = server
                .text(&["list-keys", "-T", table])
                .lines()
                .filter(|line| line.ends_with(&ending))
                .filter_map(|line| line.split_whitespace().nth(3))
                .map(str::to_string)
                .collect();
            keys.sort();
            keys
        }

        fn binding_of(server: &TmuxTestServer, table: &str, key: &str) -> String {
            let listed = server.text(&["list-keys", "-T", table]);
            let line = listed
                .lines()
                .map(|line| line.split_whitespace().collect::<Vec<_>>())
                .find(|words| words.get(3) == Some(&key))
                .unwrap_or_default();
            line.join(" ")
        }

        fn option(server: &TmuxTestServer, name: &str) -> String {
            server.text(&["show-options", "-g", "-v", name])
        }
    }

    #[test]
    fn the_state_gives_the_version_and_the_session_count() {
        let server = TmuxTestServer::with_session("holder");
        server.start_session("second");
        let tmux = server.command();

        let state = PrivateServer::new(&tmux).state().unwrap().unwrap();

        assert!(state.version().starts_with('3'));
        assert_eq!(state.sessions(), 2);
    }

    #[test]
    fn a_live_server_without_sessions_holds_zero_sessions() {
        let server = TmuxTestServer::with_session("holder");
        server.text(&["set-option", "-s", "exit-empty", "off"]);
        server.text(&["kill-session", "-t", "=holder:"]);
        let tmux = server.command();

        let state = PrivateServer::new(&tmux).state().unwrap().unwrap();

        assert_eq!(state.sessions(), 0);
    }

    #[test]
    fn an_absent_server_has_no_state_and_no_config() {
        let server = TmuxTestServer::new();
        let tmux = server.command();
        let private = PrivateServer::new(&tmux);

        assert_eq!(private.state().unwrap(), None);
        assert_eq!(private.config(&KeyBindings::default()).unwrap(), None);
    }

    #[test]
    fn a_socket_without_access_is_an_error_and_not_an_absent_server() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        fs::set_permissions(server.directory(), fs::Permissions::from_mode(0o000)).unwrap();

        let state = PrivateServer::new(&tmux).state();

        fs::set_permissions(server.directory(), fs::Permissions::from_mode(0o700)).unwrap();
        assert!(state.is_err(), "{state:?}");
    }

    #[test]
    fn a_server_whose_socket_was_removed_gets_it_back_with_its_sessions() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        fs::remove_file(server.socket()).unwrap();

        let restored = PrivateServer::new(&tmux).restore_socket().unwrap();

        assert!(restored);
        assert!(
            server
                .text(&["list-sessions", "-F", "#{session_name}"])
                .contains("holder")
        );
    }

    #[test]
    fn a_removed_socket_directory_comes_back_private() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        fs::remove_dir_all(server.directory()).unwrap();

        let restored = PrivateServer::new(&tmux).restore_socket().unwrap();

        let mode = fs::metadata(server.directory())
            .unwrap()
            .permissions()
            .mode();
        assert!(restored);
        assert_eq!(mode & 0o777, 0o700);
    }

    #[test]
    fn a_server_starting_under_the_start_lock_is_left_to_bind_its_socket() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        let private = PrivateServer::new(&tmux);
        fs::remove_file(server.socket()).unwrap();
        let lock = ServerFixture::start_lock(&server);
        lock.lock().unwrap();

        let lost = private.socket_lost();
        let restored = private.restore_socket().unwrap();

        drop(lock);
        private.restore_socket().unwrap();
        assert!(!lost);
        assert!(!restored);
    }

    #[test]
    fn a_start_lock_left_behind_does_not_stop_the_recovery() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        fs::remove_file(server.socket()).unwrap();
        drop(ServerFixture::start_lock(&server));

        assert!(PrivateServer::new(&tmux).restore_socket().unwrap());
    }

    #[test]
    fn a_server_still_on_its_socket_needs_no_restore() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();

        assert!(!PrivateServer::new(&tmux).restore_socket().unwrap());
    }

    #[test]
    fn no_server_and_no_socket_needs_no_restore() {
        let server = TmuxTestServer::new();
        let tmux = server.command();

        assert!(!PrivateServer::new(&tmux).restore_socket().unwrap());
    }

    #[test]
    fn a_server_started_with_the_render_matches_it() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();

        let running = PrivateServer::new(&tmux)
            .config(&KeyBindings::default())
            .unwrap()
            .unwrap();

        assert!(running.is_current());
        assert_eq!(running.key_bindings(), Some(&KeyBindings::default()));
    }

    #[test]
    fn moving_the_return_key_off_a_key_copy_mode_binds_gives_that_binding_back() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        let private = PrivateServer::new(&tmux);

        private.apply(&ServerFixture::alt_x()).unwrap();
        private.apply(&KeyBindings::default()).unwrap();

        for table in ["copy-mode", "copy-mode-vi"] {
            assert_eq!(
                ServerFixture::binding_of(&server, table, "M-x"),
                format!("bind-key -T {table} M-x send-keys -X jump-to-mark")
            );
        }
        assert_eq!(ServerFixture::detach_keys(&server), ["C-q"]);
    }

    #[test]
    fn moving_the_return_key_off_a_key_with_a_braced_binding_replays_it_whole() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        let private = PrivateServer::new(&tmux);
        let original = ServerFixture::binding_of(&server, "copy-mode", "C-r");
        assert!(!original.is_empty());
        let alt_r = KeyBindings::default().with_return_key(HotKey::parse("C-r").unwrap());

        private.apply(&alt_r).unwrap();
        private.apply(&KeyBindings::default()).unwrap();

        assert_eq!(
            ServerFixture::binding_of(&server, "copy-mode", "C-r"),
            original
        );
    }

    #[test]
    fn dropping_a_session_key_copy_mode_binds_gives_that_binding_back() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        let private = PrivateServer::new(&tmux);
        let original = ServerFixture::binding_of(&server, "copy-mode", "M-x");
        assert!(!original.is_empty());
        let next_on_alt_x =
            KeyBindings::default().with_next_session_keys(HotKey::parse_valid(["M-x"]));

        private.apply(&next_on_alt_x).unwrap();
        private.apply(&KeyBindings::default()).unwrap();

        assert_eq!(
            ServerFixture::binding_of(&server, "copy-mode", "M-x"),
            original
        );
    }

    #[test]
    fn applying_another_return_key_rebinds_and_records_the_new_render() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        let private = PrivateServer::new(&tmux);
        let before = private.config(&ServerFixture::alt_x()).unwrap().unwrap();

        private.apply(&ServerFixture::alt_x()).unwrap();

        let after = private.config(&ServerFixture::alt_x()).unwrap().unwrap();
        let written = fs::read_to_string(server.config()).unwrap();
        assert!(!before.is_current());
        assert!(after.is_current());
        assert_eq!(ServerFixture::detach_keys(&server), ["M-x"]);
        assert_eq!(ServerFixture::detach_keys_in(&server, "copy-mode"), ["M-x"]);
        assert!(written.contains("bind-key -T root 'M-x' detach-client"));
        assert_eq!(
            ServerFixture::option(&server, "@ringleader_config"),
            TmuxServerConfig::new(&ServerFixture::alt_x()).hash()
        );
    }

    #[test]
    fn applying_a_changed_next_session_key_unbinds_the_old_one_in_every_table() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();

        PrivateServer::new(&tmux)
            .apply(&ServerFixture::alt_n_and_alt_down())
            .unwrap();

        for table in ServerFixture::TABLES {
            let switching = ServerFixture::keys_running(&server, table, "switch-client -n");
            assert_eq!(switching, ["M-Down", "M-n"], "{table}");
            assert_eq!(
                ServerFixture::detach_keys_in(&server, table),
                ["C-q"],
                "{table}"
            );
        }
    }

    #[test]
    fn dropping_alt_up_and_alt_down_gives_emacs_copy_mode_its_halfpage_keys_back() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        let without_arrows = KeyBindings::default()
            .with_next_session_keys(HotKey::parse_valid(["M-j"]))
            .with_previous_session_keys(HotKey::parse_valid(["M-k"]));

        PrivateServer::new(&tmux).apply(&without_arrows).unwrap();

        let halfpage = |action: &str| {
            ServerFixture::keys_running(&server, "copy-mode", &format!("send-keys -X {action}"))
        };
        assert_eq!(halfpage("halfpage-up"), ["M-Up"]);
        assert_eq!(halfpage("halfpage-down"), ["M-Down"]);
    }

    #[test]
    fn applying_changed_session_keys_records_the_new_hash_and_options() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        let changed = ServerFixture::alt_n_and_alt_down();

        PrivateServer::new(&tmux).apply(&changed).unwrap();

        assert_eq!(
            ServerFixture::option(&server, "@ringleader_config"),
            TmuxServerConfig::new(&changed).hash()
        );
        assert_eq!(
            ServerFixture::option(&server, "@ringleader_next_session_keys"),
            "M-n M-Down"
        );
        assert_eq!(
            ServerFixture::option(&server, "@ringleader_previous_session_keys"),
            "M-k M-Up"
        );
    }
}
