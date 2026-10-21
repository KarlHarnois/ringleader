use std::fs;
use std::io;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use super::key_switch::KeySwitch;
use super::running_config::RunningConfig;
use super::tmux_command::TmuxCommand;
use super::tmux_error::TmuxError;
use super::tmux_launch::{Launched, TmuxLaunch};
use super::tmux_listing::TmuxListing;
use super::tmux_output::TmuxOutput;
use super::tmux_scripts::TmuxScripts;
use super::tmux_sequence::TmuxSequence;
use super::tmux_server_config::TmuxServerConfig;
use crate::adapters::host::live_process::LiveProcess;
use crate::adapters::host::process_table::ProcessTable;
use crate::application::ports::{MultiplexerError, ServerConfig, ServerState};
use crate::domain::KeyBindings;

pub struct PrivateServer<'a> {
    tmux: &'a TmuxCommand,
    processes: ProcessTable,
}

impl<'a> PrivateServer<'a> {
    const RESTORE_LIMIT: Duration = Duration::from_secs(2);
    const RESTORE_POLL: Duration = Duration::from_millis(20);

    pub fn new(tmux: &'a TmuxCommand, processes: ProcessTable) -> Self {
        Self { tmux, processes }
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
        let found = self.servers_without_socket();
        if found.is_empty() {
            return Ok(false);
        }
        let server = self.recorded_server(&found)?;
        self.tmux.server().prepare_socket_directory()?;
        server.signal(libc::SIGUSR1).map_err(|error| {
            self.socket_not_restored(&format!(
                "SIGUSR1 to tmux server {} failed: {error}",
                server.pid()
            ))
        })?;
        if self.socket_returns() {
            return Ok(true);
        }
        Err(self.socket_not_restored(&format!(
            "the server did not recreate it within {} seconds",
            Self::RESTORE_LIMIT.as_secs()
        )))
    }

    fn servers_without_socket(&self) -> Vec<LiveProcess> {
        let files = self.tmux.server();
        match Self::is_missing(files.socket()) && !files.server_is_starting() {
            true => self.processes.tmux_servers_on(files.socket()),
            false => Vec::new(),
        }
    }

    fn recorded_server(&self, found: &[LiveProcess]) -> Result<LiveProcess, MultiplexerError> {
        let files = self.tmux.server();
        let Some(pid) = files.recorded_server_pid() else {
            return Err(self.refused(
                "no server pid is recorded, so no process was signalled",
                found,
            ));
        };
        let Some(process) = self.processes.read(pid) else {
            return Err(self.refused(
                &format!("the recorded server pid {pid} no longer runs"),
                found,
            ));
        };
        if !process.serves(files.socket()) {
            return Err(self.refused(
                &format!("the recorded pid {pid} now names another process"),
                found,
            ));
        }
        Ok(process)
    }

    fn refused(&self, reason: &str, found: &[LiveProcess]) -> MultiplexerError {
        let pids: Vec<String> = found
            .iter()
            .map(|server| server.pid().to_string())
            .collect();
        self.socket_not_restored(&format!(
            "{reason}; send SIGUSR1 by hand to the tmux server the scan found at pid {}",
            pids.join(" or ")
        ))
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
        let output = self.query_recording_pid(&TmuxScripts::server_state())?;
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
        TmuxServerConfig::new(key_bindings, self.tmux.finder_popup())
    }

    fn write_render(&self, render: &TmuxServerConfig) -> Result<(), MultiplexerError> {
        self.tmux.server().prepare(&render.render())
    }

    fn binds_return_key(&self) -> Result<bool, MultiplexerError> {
        let output = self.query_recording_pid(&TmuxScripts::root_keys())?;
        Ok(TmuxListing::new(&output).binds_detach())
    }

    fn running_config(&self) -> Result<Option<RunningConfig>, MultiplexerError> {
        let query = TmuxScripts::server_values(&RunningConfig::formats());
        let output = self.query_recording_pid(&query)?;
        if let Some(running) = RunningConfig::read(&output) {
            return Ok(Some(running));
        }
        if output.reports_no_server() {
            return Ok(None);
        }
        Err(TmuxError::failed("display-message", &output).into())
    }

    fn query_recording_pid(&self, query: &TmuxSequence) -> Result<TmuxOutput, MultiplexerError> {
        let output = self.tmux.query(query)?;
        if let Some(pid) = TmuxListing::new(&output).server_pid() {
            self.tmux.server().record_server_pid(pid);
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::adapters::tmux::server_files::ServerFiles;
    use crate::domain::HotKey;
    use crate::test_support::TmuxTestServer;

    const BEFORE_SHELL_TOGGLE: &str =
        include_str!("../../../tests/fixtures/tmux-server-before-shell-toggle.conf");
    const BEFORE_FINDER: &str =
        include_str!("../../../tests/fixtures/tmux-server-before-finder.conf");

    struct ServerFixture;

    impl ServerFixture {
        const TABLES: [&'static str; 3] = ["root", "copy-mode", "copy-mode-vi"];
        const SHELL_TOGGLE_MARKER: &'static str = "@ringleader_meta";
        const FINDER_MARKER: &'static str = "display-popup";
        const NEWER_EXECUTABLE: &'static str = "/nix/store/example-newer/bin/ringleader";

        fn alt_x() -> KeyBindings {
            KeyBindings::default().with_return_key(HotKey::parse("M-x").unwrap())
        }

        fn alt_n_and_alt_down() -> KeyBindings {
            KeyBindings::default().with_next_session_keys(HotKey::parse_valid(["M-n", "M-Down"]))
        }

        fn record_server_pid(server: &TmuxTestServer) {
            PrivateServer::new(&server.command(), server.process_table())
                .state()
                .unwrap();
        }

        fn restore_by_hand(server: &TmuxTestServer, pid: &str) {
            std::process::Command::new("kill")
                .args(["-USR1", pid])
                .status()
                .unwrap();
            assert!(server.wait_for(|| server.socket().exists()));
        }

        fn exited_pid() -> u32 {
            let mut child = std::process::Command::new("true").spawn().unwrap();
            child.wait().unwrap();
            child.id()
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
            let mut keys = Self::keys_bound_where(server, table, |line| line.ends_with(&ending));
            keys.sort();
            keys
        }

        fn toggle_keys_in(server: &TmuxTestServer, table: &str) -> Vec<String> {
            Self::keys_bound_where(server, table, |line| {
                line.contains(Self::SHELL_TOGGLE_MARKER)
            })
        }

        fn finder_keys_in(server: &TmuxTestServer, table: &str) -> Vec<String> {
            Self::keys_bound_where(server, table, |line| line.contains(Self::FINDER_MARKER))
        }

        fn command_running(server: &TmuxTestServer, executable: &str) -> TmuxCommand {
            let files = ServerFiles::new(server.socket(), server.config());
            let finder = TmuxTestServer::stand_in_finder_at(executable);
            TmuxCommand::new(server.program(), &files, &server.environment(), &finder)
        }

        fn keys_bound_where(
            server: &TmuxTestServer,
            table: &str,
            matches: impl Fn(&str) -> bool,
        ) -> Vec<String> {
            server
                .text(&["list-keys", "-T", table])
                .lines()
                .filter(|line| matches(line))
                .filter_map(|line| line.split_whitespace().nth(3))
                .map(str::to_string)
                .collect()
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

        fn command_with_shell(server: &TmuxTestServer, shell: &str) -> TmuxCommand {
            server.command_with(&server.environment_with(&[("SHELL", shell)]))
        }
    }

    #[test]
    fn the_state_gives_the_version_and_the_session_count() {
        let server = TmuxTestServer::with_session("holder");
        server.start_session("second");
        let tmux = server.command();

        let state = PrivateServer::new(&tmux, server.process_table())
            .state()
            .unwrap()
            .unwrap();

        assert!(state.version().starts_with('3'));
        assert_eq!(state.sessions(), 2);
    }

    #[test]
    fn a_live_server_without_sessions_holds_zero_sessions() {
        let server = TmuxTestServer::with_session("holder");
        server.text(&["set-option", "-s", "exit-empty", "off"]);
        server.text(&["kill-session", "-t", "=holder:"]);
        let tmux = server.command();

        let state = PrivateServer::new(&tmux, server.process_table())
            .state()
            .unwrap()
            .unwrap();

        assert_eq!(state.sessions(), 0);
    }

    #[test]
    fn an_absent_server_has_no_state_and_no_config() {
        let server = TmuxTestServer::new();
        let tmux = server.command();
        let private = PrivateServer::new(&tmux, server.process_table());

        assert_eq!(private.state().unwrap(), None);
        assert_eq!(private.config(&KeyBindings::default()).unwrap(), None);
    }

    #[test]
    fn a_socket_without_access_is_an_error_and_not_an_absent_server() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        fs::set_permissions(server.directory(), fs::Permissions::from_mode(0o000)).unwrap();

        let state = PrivateServer::new(&tmux, server.process_table()).state();

        fs::set_permissions(server.directory(), fs::Permissions::from_mode(0o700)).unwrap();
        assert!(state.is_err(), "{state:?}");
    }

    #[test]
    fn a_recorded_server_whose_socket_was_removed_gets_it_back_with_its_sessions() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        ServerFixture::record_server_pid(&server);
        fs::remove_file(server.socket()).unwrap();

        let restored = PrivateServer::new(&tmux, server.process_table())
            .restore_socket()
            .unwrap();

        assert!(restored);
        assert!(
            server
                .text(&["list-sessions", "-F", "#{session_name}"])
                .contains("holder")
        );
    }

    #[test]
    fn a_server_whose_pid_was_never_recorded_is_refused_a_restore_that_names_its_pid() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        let pid = server.server_pid();
        fs::remove_file(server.socket()).unwrap();

        let error = PrivateServer::new(&tmux, server.process_table())
            .restore_socket()
            .unwrap_err();

        assert!(
            error.to_string().contains("no server pid is recorded"),
            "{error}"
        );
        assert!(error.to_string().contains("SIGUSR1"), "{error}");
        assert!(error.to_string().contains(&format!("pid {pid}")), "{error}");
        assert!(!server.socket().exists());
        ServerFixture::restore_by_hand(&server, &pid);
    }

    #[test]
    fn a_recorded_pid_that_no_longer_runs_is_refused_a_restore() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        let pid = server.server_pid();
        tmux.server().record_server_pid(ServerFixture::exited_pid());
        fs::remove_file(server.socket()).unwrap();

        let error = PrivateServer::new(&tmux, server.process_table())
            .restore_socket()
            .unwrap_err();

        assert!(error.to_string().contains("no longer runs"), "{error}");
        assert!(!server.socket().exists());
        ServerFixture::restore_by_hand(&server, &pid);
    }

    #[test]
    fn a_stale_pid_that_now_names_another_process_is_refused_a_restore() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        let pid = server.server_pid();
        tmux.server().record_server_pid(std::process::id());
        fs::remove_file(server.socket()).unwrap();

        let error = PrivateServer::new(&tmux, server.process_table())
            .restore_socket()
            .unwrap_err();

        assert!(
            error.to_string().contains("now names another process"),
            "{error}"
        );
        assert!(!server.socket().exists());
        ServerFixture::restore_by_hand(&server, &pid);
    }

    #[test]
    fn a_removed_socket_directory_comes_back_private() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        ServerFixture::record_server_pid(&server);
        fs::remove_dir_all(server.socket_directory()).unwrap();

        let restored = PrivateServer::new(&tmux, server.process_table())
            .restore_socket()
            .unwrap();

        let mode = fs::metadata(server.socket_directory())
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
        let private = PrivateServer::new(&tmux, server.process_table());
        ServerFixture::record_server_pid(&server);
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
        ServerFixture::record_server_pid(&server);
        fs::remove_file(server.socket()).unwrap();
        drop(ServerFixture::start_lock(&server));

        assert!(
            PrivateServer::new(&tmux, server.process_table())
                .restore_socket()
                .unwrap()
        );
    }

    #[test]
    fn a_server_still_on_its_socket_needs_no_restore() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();

        assert!(
            !PrivateServer::new(&tmux, server.process_table())
                .restore_socket()
                .unwrap()
        );
    }

    #[test]
    fn no_server_and_no_socket_needs_no_restore() {
        let server = TmuxTestServer::new();
        let tmux = server.command();

        assert!(
            !PrivateServer::new(&tmux, server.process_table())
                .restore_socket()
                .unwrap()
        );
    }

    #[test]
    fn reading_the_state_of_a_live_server_records_its_pid_beside_the_config() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();

        PrivateServer::new(&tmux, server.process_table())
            .state()
            .unwrap();

        assert_eq!(
            tmux.server().recorded_server_pid(),
            Some(server.server_pid().parse::<u32>().unwrap())
        );
    }

    #[test]
    fn a_server_started_with_the_render_matches_it() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();

        let running = PrivateServer::new(&tmux, server.process_table())
            .config(&KeyBindings::default())
            .unwrap()
            .unwrap();

        assert!(running.is_current());
        assert_eq!(running.key_bindings(), Some(&KeyBindings::default()));
    }

    #[test]
    fn a_config_applied_under_one_shell_is_current_under_another() {
        let server = TmuxTestServer::with_session("holder");
        let applier = ServerFixture::command_with_shell(&server, "/bin/example-bash");
        let checker = ServerFixture::command_with_shell(&server, "/bin/example-zsh");
        PrivateServer::new(&applier, server.process_table())
            .apply(&KeyBindings::default())
            .unwrap();

        let running = PrivateServer::new(&checker, server.process_table())
            .config(&KeyBindings::default())
            .unwrap()
            .unwrap();

        assert!(running.is_current());
    }

    #[test]
    fn moving_the_return_key_off_a_key_copy_mode_binds_gives_that_binding_back() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        let private = PrivateServer::new(&tmux, server.process_table());

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
        let private = PrivateServer::new(&tmux, server.process_table());
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
    fn moving_the_return_key_off_page_up_gives_copy_mode_its_page_key_back() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        let private = PrivateServer::new(&tmux, server.process_table());
        let original = ServerFixture::binding_of(&server, "copy-mode", "PPage");
        assert!(!original.is_empty());
        let page_up = KeyBindings::default().with_return_key(HotKey::parse("PageUp").unwrap());

        private.apply(&page_up).unwrap();
        private.apply(&KeyBindings::default()).unwrap();

        assert_eq!(
            ServerFixture::binding_of(&server, "copy-mode", "PPage"),
            original
        );
    }

    #[test]
    fn a_return_key_parse_now_refuses_is_still_unbound_when_the_key_changes() {
        let recorded = HotKey::recorded("M-BSpace").unwrap();
        let older = KeyBindings::default().with_return_key(recorded);
        let server = TmuxTestServer::with_config(
            &TmuxServerConfig::new(&older, &TmuxTestServer::stand_in_finder()).render(),
        );
        server.start_session("holder");
        let tmux = server.command();

        PrivateServer::new(&tmux, server.process_table())
            .apply(&KeyBindings::default())
            .unwrap();

        assert_eq!(ServerFixture::detach_keys(&server), ["C-q"]);
    }

    #[test]
    fn dropping_a_session_key_copy_mode_binds_gives_that_binding_back() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        let private = PrivateServer::new(&tmux, server.process_table());
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
        let private = PrivateServer::new(&tmux, server.process_table());
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
            TmuxServerConfig::new(&ServerFixture::alt_x(), &TmuxTestServer::stand_in_finder())
                .hash()
        );
    }

    #[test]
    fn applying_a_changed_next_session_key_unbinds_the_old_one_in_every_table() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();

        PrivateServer::new(&tmux, server.process_table())
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

        PrivateServer::new(&tmux, server.process_table())
            .apply(&without_arrows)
            .unwrap();

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

        PrivateServer::new(&tmux, server.process_table())
            .apply(&changed)
            .unwrap();

        assert_eq!(
            ServerFixture::option(&server, "@ringleader_config"),
            TmuxServerConfig::new(&changed, &TmuxTestServer::stand_in_finder()).hash()
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

    #[test]
    fn applying_a_changed_shell_toggle_key_moves_the_toggle_in_every_table() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        let changed = KeyBindings::default().with_shell_toggle_keys(HotKey::parse_valid(["M-o"]));

        PrivateServer::new(&tmux, server.process_table())
            .apply(&changed)
            .unwrap();

        for table in ServerFixture::TABLES {
            assert_eq!(
                ServerFixture::toggle_keys_in(&server, table),
                ["M-o"],
                "{table}"
            );
        }
        assert_eq!(
            ServerFixture::option(&server, "@ringleader_shell_toggle_keys"),
            "M-o"
        );
    }

    #[test]
    fn applying_no_shell_toggle_keys_leaves_no_toggle_in_any_table() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        let without_toggle = KeyBindings::default().with_shell_toggle_keys(Vec::new());

        PrivateServer::new(&tmux, server.process_table())
            .apply(&without_toggle)
            .unwrap();

        for table in ServerFixture::TABLES {
            assert!(
                ServerFixture::toggle_keys_in(&server, table).is_empty(),
                "{table}"
            );
        }
    }

    #[test]
    fn a_server_started_before_the_shell_toggle_gains_it_when_the_config_is_reapplied() {
        let server = TmuxTestServer::with_config(BEFORE_SHELL_TOGGLE);
        server.start_session("holder");
        let tmux = server.command();

        PrivateServer::new(&tmux, server.process_table())
            .apply(&KeyBindings::default())
            .unwrap();

        for table in ServerFixture::TABLES {
            assert_eq!(
                ServerFixture::toggle_keys_in(&server, table),
                ["M-l"],
                "{table}"
            );
        }
    }

    #[test]
    fn applying_a_changed_finder_key_moves_the_finder_in_every_table() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        let changed = KeyBindings::default().with_finder_keys(HotKey::parse_valid(["M-f"]));

        PrivateServer::new(&tmux, server.process_table())
            .apply(&changed)
            .unwrap();

        for table in ServerFixture::TABLES {
            assert_eq!(
                ServerFixture::finder_keys_in(&server, table),
                ["M-f"],
                "{table}"
            );
        }
        assert_eq!(
            ServerFixture::option(&server, "@ringleader_finder_keys"),
            "M-f"
        );
    }

    #[test]
    fn applying_no_finder_keys_leaves_no_finder_in_any_table() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        let without_finder = KeyBindings::default().with_finder_keys(Vec::new());

        PrivateServer::new(&tmux, server.process_table())
            .apply(&without_finder)
            .unwrap();

        for table in ServerFixture::TABLES {
            assert!(
                ServerFixture::finder_keys_in(&server, table).is_empty(),
                "{table}"
            );
        }
    }

    #[test]
    fn dropping_ctrl_k_gives_emacs_copy_mode_its_copy_to_the_end_of_the_line_back() {
        let server = TmuxTestServer::with_session("holder");
        let tmux = server.command();
        let without_finder = KeyBindings::default().with_finder_keys(Vec::new());

        PrivateServer::new(&tmux, server.process_table())
            .apply(&without_finder)
            .unwrap();

        assert_eq!(
            ServerFixture::keys_running(
                &server,
                "copy-mode",
                "send-keys -X copy-pipe-end-of-line-and-cancel"
            ),
            ["C-k"]
        );
    }

    #[test]
    fn a_server_started_before_the_finder_gains_it_when_the_config_is_reapplied() {
        let server = TmuxTestServer::with_config(BEFORE_FINDER);
        server.start_session("holder");
        let tmux = server.command();

        PrivateServer::new(&tmux, server.process_table())
            .apply(&KeyBindings::default())
            .unwrap();

        for table in ServerFixture::TABLES {
            assert_eq!(
                ServerFixture::finder_keys_in(&server, table),
                ["C-k"],
                "{table}"
            );
        }
    }

    #[test]
    fn a_config_applied_by_another_executable_is_stale_so_the_finder_follows_upgrades() {
        let server = TmuxTestServer::with_session("holder");
        let newer = ServerFixture::command_running(&server, ServerFixture::NEWER_EXECUTABLE);

        let running = PrivateServer::new(&newer, server.process_table())
            .config(&KeyBindings::default())
            .unwrap()
            .unwrap();

        assert!(!running.is_current());
    }
}
