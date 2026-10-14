use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;

use crate::adapters::Deadlines;
use crate::adapters::agent_handoff::AgentHandoff;
use crate::adapters::agent_launcher::LauncherCommand;
use crate::adapters::process_environment::ProcessEnvironment;
use crate::adapters::server_files::ServerFiles;
use crate::adapters::tmux_command::TmuxCommand;
use crate::adapters::tmux_multiplexer::TmuxMultiplexer;
use crate::adapters::tmux_server_config::TmuxServerConfig;
use crate::application::ports::{AgentLaunch, Multiplexer, SessionSpec};
use crate::domain::{KeyBindings, RepoName, SessionMetadata, SessionRepo, SessionWorktree};

pub struct TmuxTestServer {
    directory: TempDir,
    files: ServerFiles,
}

impl TmuxTestServer {
    pub const STAND_IN: [&'static str; 4] = ["sh", "-c", "sleep 30", "--"];
    const SETTLE_LIMIT: Duration = Duration::from_secs(2);
    const POLL_INTERVAL: Duration = Duration::from_millis(20);
    const OUTER_SESSION: &'static str = "outer";

    pub fn new() -> Self {
        Self::with_config(&TmuxServerConfig::new(&KeyBindings::default()).render())
    }

    pub fn with_config(config_text: &str) -> Self {
        let directory = tempfile::Builder::new()
            .prefix("rl")
            .tempdir_in("/tmp")
            .expect("a socket directory under /tmp");
        let files = ServerFiles::new(
            &directory.path().join("tmux.sock"),
            &directory.path().join("tmux.conf"),
        );
        std::fs::write(files.config(), config_text).expect("the test server config is written");
        Self { directory, files }
    }

    pub fn with_session(name: &str) -> Self {
        let server = Self::new();
        server.start_session(name);
        server
    }

    pub fn socket(&self) -> &Path {
        self.files.socket()
    }

    pub fn config(&self) -> &Path {
        self.files.config()
    }

    pub fn directory(&self) -> &Path {
        self.directory.path()
    }

    pub fn environment(&self) -> ProcessEnvironment {
        self.environment_with(&[])
    }

    pub fn environment_with(&self, extra: &[(&str, &str)]) -> ProcessEnvironment {
        let mut variables = vec![(
            OsString::from("PATH"),
            std::env::var_os("PATH").unwrap_or_default(),
        )];
        variables.push((
            OsString::from("HOME"),
            self.directory().as_os_str().to_os_string(),
        ));
        variables.extend(
            extra
                .iter()
                .map(|(name, value)| (OsString::from(name), OsString::from(value))),
        );
        ProcessEnvironment::new(variables, None)
    }

    pub fn command(&self) -> TmuxCommand {
        self.command_with(&self.environment())
    }

    pub fn command_with(&self, environment: &ProcessEnvironment) -> TmuxCommand {
        self.command_with_deadlines(environment, Deadlines::standard())
    }

    pub fn command_with_deadlines(
        &self,
        environment: &ProcessEnvironment,
        deadlines: Deadlines,
    ) -> TmuxCommand {
        TmuxCommand::new(&OsString::from("tmux"), &self.files, environment)
            .with_deadlines(deadlines)
    }

    pub fn server_pid(&self) -> String {
        self.text(&["display-message", "-p", "#{pid}"])
    }

    pub fn pane_of(&self, target: &str) -> String {
        let output = self.tmux(&["display-message", "-p", "-t", target, "#{pane_id}"]);
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .to_string()
    }

    pub fn multiplexer(&self) -> TmuxMultiplexer {
        TmuxMultiplexer::new(self.command(), self.stand_in())
    }

    pub fn stand_in(&self) -> LauncherCommand {
        self.launcher(Self::STAND_IN.iter().map(OsString::from).collect())
    }

    pub fn launcher(&self, prefix: Vec<OsString>) -> LauncherCommand {
        let handoff = AgentHandoff::new(&self.handoff_directory(), &self.environment());
        LauncherCommand::new(prefix, handoff)
    }

    pub fn handoff_directory(&self) -> PathBuf {
        self.directory().join("handoff")
    }

    pub fn handoff_files(&self) -> usize {
        std::fs::read_dir(self.handoff_directory()).map_or(0, |entries| entries.count())
    }

    pub fn tmux(&self, args: &[&str]) -> Output {
        self.typed(args)
            .stdin(Stdio::null())
            .output()
            .expect("tmux runs")
    }

    pub fn source(&self, config_text: &str) -> Output {
        let mut child = self
            .typed(&["source-file", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("tmux runs");
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(config_text.as_bytes())
                .expect("tmux reads the config text");
        }
        child.wait_with_output().expect("tmux finishes")
    }

    pub fn text(&self, args: &[&str]) -> String {
        let output = self.tmux(args);
        assert!(
            output.status.success(),
            "tmux {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_string()
    }

    pub fn managed_session(&self, name: &str) -> PathBuf {
        self.managed_session_through(&self.multiplexer(), name)
    }

    pub fn managed_session_through(&self, multiplexer: &TmuxMultiplexer, name: &str) -> PathBuf {
        let worktree = self.directory().join(name);
        std::fs::create_dir_all(&worktree).expect("the worktree directory is created");
        let metadata = SessionMetadata::new(
            SessionRepo::new(
                RepoName::parse("example-project").expect("a valid repo name"),
                &worktree,
            ),
            SessionWorktree::new(&worktree, &format!("ringleader/{name}"), ""),
            "stand-in",
        );
        let launch = AgentLaunch::new(Vec::new(), &[], &worktree);
        let spec = SessionSpec::new(&format!("ringleader-{name}"), metadata, launch);
        multiplexer
            .create_session(&spec)
            .expect("the managed session starts");
        worktree
    }

    pub fn nested_client(&self, target: &str) -> TmuxTestServer {
        let outer = TmuxTestServer::new();
        let socket = self.socket().display().to_string();
        let config = self.config().display().to_string();
        outer.text(&[
            "new-session",
            "-d",
            "-s",
            Self::OUTER_SESSION,
            "env",
            "-u",
            "TMUX",
            "-u",
            "TMUX_PANE",
            "tmux",
            "-u",
            "-S",
            &socket,
            "-f",
            &config,
            "attach-session",
            "-t",
            target,
        ]);
        outer
    }

    pub fn type_bytes(&self, typed: &[&str]) {
        let outer = format!("={}:", Self::OUTER_SESSION);
        let mut typing = vec!["send-keys", "-t", &outer, "-H"];
        typing.extend(typed);
        self.text(&typing);
    }

    pub fn start_session(&self, name: &str) {
        let mut args = vec!["new-session", "-d", "-s", name];
        args.extend(Self::STAND_IN);
        self.text(&args);
    }

    pub fn wait_for(&self, condition: impl Fn() -> bool) -> bool {
        let deadline = Instant::now() + Self::SETTLE_LIMIT;
        while Instant::now() < deadline {
            if condition() {
                return true;
            }
            thread::sleep(Self::POLL_INTERVAL);
        }
        condition()
    }

    pub fn control_client(&self, target: &str) -> ControlClient {
        let mut child = self
            .typed(&["-C", "attach-session", "-t", target])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("a control-mode client");
        let stdin = child.stdin.take();
        let output = ControlClient::read_output(&mut child);
        let client = ControlClient {
            child,
            _stdin: stdin,
            output,
        };
        let pid = client.child.id().to_string();
        assert!(
            self.wait_for(|| self.client_named_by_pid(&pid).is_some()),
            "the control-mode client attached"
        );
        client
    }

    pub fn client_name(&self, client: &ControlClient) -> String {
        self.client_named_by_pid(&client.child.id().to_string())
            .expect("the control-mode client is attached")
    }

    pub fn client_pane(&self, client: &ControlClient) -> String {
        let name = self.client_name(client);
        let listing = self.text(&["list-clients", "-F", "#{client_name} #{pane_id}"]);
        listing
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{name} ")))
            .unwrap_or_default()
            .to_string()
    }

    pub fn press(&self, client: &ControlClient, key: &str) {
        let name = self.client_name(client);
        self.text(&["send-keys", "-K", "-c", &name, key]);
    }

    fn typed(&self, args: &[&str]) -> Command {
        let mut command = Command::new("tmux");
        command
            .env_clear()
            .envs(self.environment().variables().iter().cloned())
            .arg("-u")
            .arg("-S")
            .arg(self.socket())
            .arg("-f")
            .arg(self.config())
            .args(args)
            .current_dir(self.directory());
        command
    }

    fn client_named_by_pid(&self, pid: &str) -> Option<String> {
        let listing = self.tmux(&["list-clients", "-F", "#{client_pid} #{client_name}"]);
        String::from_utf8_lossy(&listing.stdout)
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{pid} ")).map(str::to_string))
    }
}

impl Drop for TmuxTestServer {
    fn drop(&mut self) {
        let _ = self.tmux(&["kill-server"]);
    }
}

pub struct ControlClient {
    child: Child,
    _stdin: Option<ChildStdin>,
    output: Receiver<String>,
}

impl ControlClient {
    pub fn exits_with_notice_within(&mut self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return self
                    .output
                    .recv_timeout(Duration::from_secs(1))
                    .is_ok_and(|text| text.contains("%exit"));
            }
            thread::sleep(Duration::from_millis(20));
        }
        false
    }

    fn read_output(child: &mut Child) -> Receiver<String> {
        let (sender, receiver) = mpsc::channel();
        if let Some(mut stdout) = child.stdout.take() {
            thread::spawn(move || {
                let mut text = String::new();
                let _ = stdout.read_to_string(&mut text);
                let _ = sender.send(text);
            });
        }
        receiver
    }
}

impl Drop for ControlClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
