use std::ffi::OsString;
use std::io::{Read, Write};
use std::iter::once;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;

use super::{InheritedPath, ProcessMarker};
use crate::adapters::Deadlines;
use crate::adapters::agent_launch::agent_handoff::AgentHandoff;
use crate::adapters::agent_launch::launcher_command::LauncherCommand;
use crate::adapters::host::process_environment::ProcessEnvironment;
use crate::adapters::host::process_table::ProcessTable;
use crate::adapters::tmux::server_files::ServerFiles;
use crate::adapters::tmux::tmux_command::TmuxCommand;
use crate::adapters::tmux::tmux_multiplexer::TmuxMultiplexer;
use crate::adapters::tmux::tmux_popups::TmuxPopups;
use crate::adapters::tmux::tmux_server_config::TmuxServerConfig;
use crate::application::ports::{AgentLaunch, Multiplexer, SessionSpec};
use crate::domain::{KeyBindings, RepoName, SessionMetadata, SessionRepo, SessionWorktree};

pub struct TmuxTestServer {
    _tempdir: TempDir,
    canonical_directory: PathBuf,
    files: ServerFiles,
    program: OsString,
    marker: ProcessMarker,
}

impl TmuxTestServer {
    pub const STAND_IN: [&'static str; 4] = ["sh", "-c", "sleep 30", "--"];
    const STAND_IN_EXECUTABLE: &'static str = "/nix/store/example-ringleader/bin/ringleader";
    const STAND_IN_FINDER_ARGUMENTS: [&'static str; 2] = ["__find-session", "--client"];
    const STAND_IN_MENU_ARGUMENTS: [&'static str; 2] = ["__session-menu", "--client"];
    const STAND_IN_PROGRAMS: [&'static str; 2] = [
        "RINGLEADER_TMUX=/nix/store/example-tmux/bin/tmux",
        "RINGLEADER_GIT=/nix/store/example-git/bin/git",
    ];
    const SETTLE_LIMIT: Duration = Duration::from_secs(2);
    const POLL_INTERVAL: Duration = Duration::from_millis(20);
    const OUTER_SESSION: &'static str = "outer";

    pub fn new() -> Self {
        Self::with_config(&Self::default_config())
    }

    pub fn with_config(config_text: &str) -> Self {
        Self::with_config_marked(config_text, ProcessMarker::new())
    }

    pub fn sharing_table_with(other: &Self) -> Self {
        Self::with_config_marked(&Self::default_config(), other.marker.clone())
    }

    fn default_config() -> String {
        TmuxServerConfig::new(&KeyBindings::default(), &Self::stand_in_popups()).render()
    }

    pub fn stand_in_popups() -> TmuxPopups {
        Self::stand_in_popups_at(Self::STAND_IN_EXECUTABLE)
    }

    pub fn stand_in_popups_at(executable: &str) -> TmuxPopups {
        TmuxPopups::new(
            Self::stand_in_prefix(executable, Self::STAND_IN_FINDER_ARGUMENTS),
            Self::stand_in_prefix(executable, Self::STAND_IN_MENU_ARGUMENTS),
            &Self::STAND_IN_PROGRAMS.map(OsString::from),
        )
    }

    fn stand_in_prefix(executable: &str, arguments: [&str; 2]) -> Vec<OsString> {
        once(executable)
            .chain(arguments)
            .map(OsString::from)
            .collect()
    }

    fn with_config_marked(config_text: &str, marker: ProcessMarker) -> Self {
        let tempdir = tempfile::Builder::new()
            .prefix("rl")
            .tempdir_in("/tmp")
            .expect("a socket directory under /tmp");
        let canonical_directory =
            std::fs::canonicalize(tempdir.path()).expect("a canonical socket directory");
        let files = ServerFiles::new(
            &canonical_directory.join("tmux").join("tmux.sock"),
            &canonical_directory.join("tmux.conf"),
        );
        std::fs::create_dir(files.socket().parent().expect("a socket directory"))
            .expect("the test server socket directory is made");
        std::fs::write(files.config(), config_text).expect("the test server config is written");
        Self {
            _tempdir: tempdir,
            canonical_directory,
            files,
            program: OsString::from("tmux"),
            marker,
        }
    }

    pub fn process_table(&self) -> ProcessTable {
        self.marker.table()
    }

    pub fn under_program_name(name: &str) -> Self {
        let mut server = Self::new();
        let program = server.directory().join(name);
        let copied = Command::new("cp")
            .arg(Self::tmux_on_path())
            .arg(&program)
            .status()
            .expect("cp runs");
        assert!(copied.success(), "a copy of tmux under another name");
        server.program = program.into_os_string();
        server
    }

    pub fn program(&self) -> &std::ffi::OsStr {
        &self.program
    }

    pub fn with_session(name: &str) -> Self {
        let server = Self::new();
        server.start_session(name);
        server
    }

    pub fn socket(&self) -> &Path {
        self.files.socket()
    }

    pub fn socket_directory(&self) -> &Path {
        self.socket().parent().expect("a socket directory")
    }

    pub fn config(&self) -> &Path {
        self.files.config()
    }

    pub fn directory(&self) -> &Path {
        &self.canonical_directory
    }

    pub fn environment(&self) -> ProcessEnvironment {
        self.environment_with(&[])
    }

    pub fn environment_with(&self, extra: &[(&str, &str)]) -> ProcessEnvironment {
        let mut variables = vec![(OsString::from("PATH"), InheritedPath::value())];
        variables.push((
            OsString::from("HOME"),
            self.directory().as_os_str().to_os_string(),
        ));
        variables.push(self.marker.variable());
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
        TmuxCommand::new(
            &self.program,
            &self.files,
            &self.marked(environment),
            &Self::stand_in_popups(),
        )
        .with_deadlines(deadlines)
    }

    fn marked(&self, environment: &ProcessEnvironment) -> ProcessEnvironment {
        if environment.get(ProcessMarker::NAME).is_some() {
            return environment.clone();
        }
        let variables = environment
            .variables()
            .iter()
            .cloned()
            .chain([self.marker.variable()])
            .collect();
        ProcessEnvironment::new(variables, environment.terminal().map(ToOwned::to_owned))
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
        TmuxMultiplexer::new(self.command(), self.stand_in(), self.process_table())
    }

    pub fn stand_in(&self) -> LauncherCommand {
        self.launcher(Self::STAND_IN.iter().map(OsString::from).collect())
    }

    pub fn launcher(&self, prefix: Vec<OsString>) -> LauncherCommand {
        let handoff = AgentHandoff::new(
            &self.handoff_directory(),
            &self.environment(),
            self.process_table(),
        );
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
        let mut command = Command::new(&self.program);
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

    fn tmux_on_path() -> PathBuf {
        InheritedPath::program("tmux").expect("tmux is on PATH")
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
