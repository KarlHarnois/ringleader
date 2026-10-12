use std::fs;
use std::io;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use super::live_process::LiveProcess;
use super::tmux_command::TmuxCommand;
use super::tmux_error::TmuxError;
use super::tmux_launch::{Launched, TmuxLaunch};
use super::tmux_listing::TmuxListing;
use super::tmux_output::TmuxOutput;
use super::tmux_scripts::TmuxScripts;
use super::tmux_server_config::TmuxServerConfig;
use crate::application::ports::{MultiplexerError, ServerConfig, ServerState};
use crate::domain::ReturnKey;

pub struct PrivateServer<'a> {
    tmux: &'a TmuxCommand,
}

impl<'a> PrivateServer<'a> {
    const RESTORE_LIMIT: Duration = Duration::from_secs(2);
    const RESTORE_POLL: Duration = Duration::from_millis(20);

    pub fn new(tmux: &'a TmuxCommand) -> Self {
        Self { tmux }
    }

    pub fn write_files(&self, return_key: &ReturnKey) -> Result<(), MultiplexerError> {
        self.write_render(&self.render_for(return_key))
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
        self.tmux
            .server()
            .prepare_socket_directory()
            .map_err(|error| MultiplexerError::ServerFiles {
                detail: error.to_string(),
            })?;
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
        let socket = self.tmux.server().socket();
        match Self::is_missing(socket) {
            true => LiveProcess::tmux_servers_on(socket),
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

    pub fn config(&self, return_key: &ReturnKey) -> Result<Option<ServerConfig>, MultiplexerError> {
        let Some(running) = self.running_config()? else {
            return Ok(None);
        };
        let current = running.hash() == self.render_for(return_key).hash();
        Ok(Some(ServerConfig::new(current, running.return_key())))
    }

    pub fn apply(&self, return_key: &ReturnKey) -> Result<(), MultiplexerError> {
        let running = self.running_config()?;
        let previous_key = running
            .as_ref()
            .and_then(RunningConfig::return_key)
            .filter(|previous| *previous != return_key);
        let render = self.render_for(return_key);
        self.write_render(&render)?;
        let sequence = TmuxServerConfig::reapply_script(previous_key);
        let output = self
            .tmux
            .change_with_input(&sequence, render.render().as_bytes())?;
        if output.succeeded() {
            return Ok(());
        }
        Err(TmuxError::failed("source-file", &output).into())
    }

    fn render_for(&self, return_key: &ReturnKey) -> TmuxServerConfig {
        TmuxServerConfig::new(return_key)
    }

    fn write_render(&self, render: &TmuxServerConfig) -> Result<(), MultiplexerError> {
        self.tmux
            .server()
            .prepare(&render.render())
            .map_err(|error| MultiplexerError::ServerFiles {
                detail: error.to_string(),
            })
    }

    fn binds_return_key(&self) -> Result<bool, MultiplexerError> {
        let output = self.tmux.query(&TmuxScripts::root_keys())?;
        Ok(TmuxListing::new(&output).binds_detach())
    }

    fn running_config(&self) -> Result<Option<RunningConfig>, MultiplexerError> {
        let query = TmuxScripts::server_value(&RunningConfig::format());
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

struct RunningConfig {
    hash: String,
    return_key: Option<ReturnKey>,
}

impl RunningConfig {
    const LABEL: &'static str = "ringleader-config";

    fn format() -> String {
        format!(
            "{} #{{{}}} #{{{}}}",
            Self::LABEL,
            TmuxServerConfig::HASH_OPTION,
            TmuxServerConfig::RETURN_KEY_OPTION
        )
    }

    fn read(output: &TmuxOutput) -> Option<Self> {
        let values = output.first(Self::LABEL)?;
        let (hash, return_key) = values.split_once(' ').unwrap_or((values, ""));
        Some(Self {
            hash: hash.to_string(),
            return_key: ReturnKey::parse(return_key),
        })
    }

    fn hash(&self) -> &str {
        &self.hash
    }

    fn return_key(&self) -> Option<&ReturnKey> {
        self.return_key.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::test_support::TmuxTestServer;

    struct ServerFixture;

    impl ServerFixture {
        fn alt_x() -> ReturnKey {
            ReturnKey::parse("M-x").unwrap()
        }

        fn detach_keys(server: &TmuxTestServer) -> Vec<String> {
            Self::detach_keys_in(server, "root")
        }

        fn detach_keys_in(server: &TmuxTestServer, table: &str) -> Vec<String> {
            server
                .text(&["list-keys", "-T", table])
                .lines()
                .filter(|line| line.ends_with(" detach-client"))
                .filter_map(|line| line.split_whitespace().nth(3))
                .map(str::to_string)
                .collect()
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
        assert_eq!(private.config(&ReturnKey::default()).unwrap(), None);
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
            .config(&ReturnKey::default())
            .unwrap()
            .unwrap();

        assert!(running.is_current());
        assert_eq!(running.return_key(), Some(&ReturnKey::default()));
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
            server.text(&["show-options", "-g", "-v", "@ringleader_config"]),
            TmuxServerConfig::new(&ServerFixture::alt_x()).hash()
        );
    }
}
