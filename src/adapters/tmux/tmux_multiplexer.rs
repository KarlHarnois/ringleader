use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use super::private_server::PrivateServer;
use super::tmux_agent_input::{InputError, TmuxAgentInput};
use super::tmux_command::TmuxCommand;
use super::tmux_error::TmuxError;
use super::tmux_launch::{Launched, TmuxLaunch};
use super::tmux_listing::TmuxListing;
use super::tmux_output::TmuxOutput;
use super::tmux_scripts::TmuxScripts;
use crate::adapters::agent_launch::launcher_command::{LauncherCommand, PreparedLaunch};
use crate::adapters::child_process::foreground_child::ForegroundChild;
use crate::adapters::host::process_table::ProcessTable;
use crate::application::ports::{
    AgentLaunch, AttachTarget, Multiplexer, MultiplexerError, ServerConfig, ServerState,
    SessionSpec,
};
use crate::domain::{KeyBindings, SessionMetadata, SessionObservation, SessionSize};

pub struct TmuxMultiplexer {
    tmux: TmuxCommand,
    launcher: LauncherCommand,
    processes: ProcessTable,
}

impl TmuxMultiplexer {
    const MISSING_SESSION: &'static str = "can't find session";
    const STARTUP_PATIENCE: Duration = Duration::from_secs(60);
    const AGENT_REDRAW_TIME: Duration = Duration::from_millis(60);
    const WINDOW_SIZE_LABEL: &'static str = "ringleader-window-size";

    pub fn new(tmux: TmuxCommand, launcher: LauncherCommand, processes: ProcessTable) -> Self {
        Self {
            tmux,
            launcher,
            processes,
        }
    }

    fn private_server(&self) -> PrivateServer<'_> {
        PrivateServer::new(&self.tmux, self.processes.clone())
    }

    fn prepare(&self, launch: &AgentLaunch) -> Result<PreparedLaunch, MultiplexerError> {
        self.launcher
            .prepare(launch)
            .map_err(|error| MultiplexerError::Handoff {
                detail: error.to_string(),
            })
    }

    fn discarding_on_failure<T>(
        prepared: &PreparedLaunch,
        result: Result<T, MultiplexerError>,
    ) -> Result<T, MultiplexerError> {
        if result.is_err() {
            prepared.discard();
        }
        result
    }

    fn start_session(
        &self,
        spec: &SessionSpec,
        prepared: &PreparedLaunch,
    ) -> Result<PathBuf, MultiplexerError> {
        let server = self.private_server();
        server.restore_socket()?;
        server.write_files(spec.key_bindings())?;
        let sequence =
            TmuxScripts::create_session(spec, prepared.words(), &self.tmux.caller_shell());
        let output = self.tmux.create(&sequence, spec.removed_env_names())?;
        let launched = TmuxLaunch::new(&self.tmux).run(&sequence, output)?;
        let socket = self.finish_creation(&launched)?;
        server.require_return_binding(&launched)?;
        Ok(socket)
    }

    fn revive_agent(
        &self,
        tmux_name: &str,
        launch: &AgentLaunch,
        prepared: &PreparedLaunch,
    ) -> Result<(), MultiplexerError> {
        let Some(pane) = self.agent_pane(tmux_name)? else {
            return self.launch_agent_window(tmux_name, launch, prepared);
        };
        let respawn = TmuxScripts::respawn(&pane, launch, prepared.words());
        let output = self.tmux.change(&respawn)?;
        Self::require_done("respawn-pane", &output)
    }

    fn finish_creation(&self, launched: &Launched) -> Result<PathBuf, MultiplexerError> {
        let launch = TmuxLaunch::new(&self.tmux);
        let values = launched.done_values();
        let [current_pane, socket] = values.as_slice() else {
            launch.roll_back(launched);
            return Err(MultiplexerError::Failed {
                subcommand: "new-session".to_string(),
                detail: "tmux printed an incomplete confirmation".to_string(),
            });
        };
        launch.settle_agent_tag(launched, current_pane)?;
        Ok(PathBuf::from(socket))
    }

    fn launch_agent_window(
        &self,
        tmux_name: &str,
        launch: &AgentLaunch,
        prepared: &PreparedLaunch,
    ) -> Result<(), MultiplexerError> {
        let sequence = TmuxScripts::agent_window(tmux_name, launch, prepared.words());
        let output = self.tmux.change(&sequence)?;
        let launcher = TmuxLaunch::new(&self.tmux);
        let launched = launcher.run(&sequence, output)?;
        let current_pane = launched.done_values().first().map(|pane| pane.to_string());
        Ok(launcher.settle_agent_tag(&launched, &current_pane.unwrap_or_default())?)
    }

    fn type_into_agent(
        &self,
        tmux_name: &str,
        lines: &[String],
        patience: Duration,
    ) -> Result<(), MultiplexerError> {
        self.reach_agent(tmux_name, patience, |input, pane| {
            input.type_when_ready(pane, lines)
        })
    }

    fn reach_agent(
        &self,
        tmux_name: &str,
        patience: Duration,
        deliver: impl FnOnce(&TmuxAgentInput, &str) -> Result<(), InputError>,
    ) -> Result<(), MultiplexerError> {
        let pane = self.required_agent_pane(tmux_name)?;
        deliver(&TmuxAgentInput::new(&self.tmux, patience), &pane)
            .map_err(|error| error.in_session(tmux_name))
    }

    fn required_agent_pane(&self, tmux_name: &str) -> Result<String, MultiplexerError> {
        self.agent_pane(tmux_name)?
            .ok_or_else(|| MultiplexerError::NoAgentPane {
                tmux_name: tmux_name.to_string(),
            })
    }

    fn agent_pane(&self, tmux_name: &str) -> Result<Option<String>, MultiplexerError> {
        self.agent_pane_in(&self.session_target(tmux_name)?)
    }

    fn agent_pane_in(&self, session: &str) -> Result<Option<String>, MultiplexerError> {
        let lookup = TmuxScripts::session_panes(session, &TmuxListing::agent_format());
        let output = self.tmux.query(&lookup)?;
        if !output.succeeded() && !output.has("ringleader-agent") {
            return Err(TmuxError::failed("list-panes", &output).into());
        }
        Ok(TmuxListing::new(&output).agent_pane())
    }

    fn attach_command(&self, target: &AttachTarget) -> Result<Command, MultiplexerError> {
        let resolved = self.resolved(target)?;
        Ok(self
            .tmux
            .foreground(&TmuxScripts::attach_session(&resolved)))
    }

    fn resolved(&self, target: &AttachTarget) -> Result<String, MultiplexerError> {
        match target {
            AttachTarget::Agent(tmux_name) => self.shown_pane(tmux_name),
            AttachTarget::Shell(window) => Ok(window.clone()),
        }
    }

    fn shown_pane(&self, tmux_name: &str) -> Result<String, MultiplexerError> {
        let session = self.session_target(tmux_name)?;
        Ok(self.agent_pane_in(&session)?.unwrap_or(session))
    }

    fn session_target(&self, tmux_name: &str) -> Result<String, MultiplexerError> {
        if !TmuxScripts::reads_as_session_id(tmux_name) {
            return Ok(TmuxScripts::exact(tmux_name));
        }
        let output = self.tmux.query(&TmuxScripts::session_ids())?;
        TmuxListing::new(&output)
            .session_id_named(tmux_name)
            .map(|session_id| TmuxScripts::by_session_id(&session_id))
            .ok_or_else(|| MultiplexerError::Failed {
                subcommand: "list-sessions".to_string(),
                detail: format!("no tmux session is named {tmux_name}"),
            })
    }

    fn attach_in_foreground(&self, mut attach: Command) -> Result<(), MultiplexerError> {
        let child = attach
            .spawn()
            .map_err(|error| Self::attach_failed(error.to_string()))?;
        ForegroundChild::of(&child).map_or(Ok(()), |foreground| {
            foreground.wait().map_err(|status| {
                Self::attach_failed(format!(
                    "tmux attach-session ended with wait status {status}"
                ))
            })
        })
    }

    fn attach_failed(detail: String) -> MultiplexerError {
        MultiplexerError::Failed {
            subcommand: "attach-session".to_string(),
            detail,
        }
    }

    fn resized(output: &TmuxOutput) -> bool {
        let mut sizes = output.labeled(Self::WINDOW_SIZE_LABEL);
        sizes.next() != sizes.next()
    }

    fn require_done(subcommand: &str, output: &TmuxOutput) -> Result<(), MultiplexerError> {
        if output.has(TmuxLaunch::DONE_LABEL) {
            return Ok(());
        }
        Err(TmuxError::failed(subcommand, output).into())
    }
}

impl Multiplexer for TmuxMultiplexer {
    fn socket(&self) -> PathBuf {
        self.tmux.server().socket().to_path_buf()
    }

    fn create_session(&self, spec: &SessionSpec) -> Result<PathBuf, MultiplexerError> {
        let prepared = self.prepare(spec.launch())?;
        Self::discarding_on_failure(&prepared, self.start_session(spec, &prepared))
    }

    fn kill_session(&self, tmux_name: &str) -> Result<(), MultiplexerError> {
        let output = self.tmux.change(&TmuxScripts::kill(tmux_name))?;
        let already_gone =
            output.stderr().contains(Self::MISSING_SESSION) || output.reports_no_server();
        if output.has(TmuxLaunch::DONE_LABEL) || already_gone {
            return Ok(());
        }
        Err(TmuxError::failed("kill-session", &output).into())
    }

    fn pane_pids(&self, tmux_name: &str) -> Result<Vec<u32>, MultiplexerError> {
        let session = self.session_target(tmux_name)?;
        let list = TmuxScripts::session_panes(&session, TmuxListing::PANE_PID_FORMAT);
        let output = self.tmux.query(&list)?;
        if !output.succeeded() && !output.has("ringleader-pane-pid") {
            return Err(TmuxError::failed("list-panes", &output).into());
        }
        Ok(TmuxListing::new(&output).live_pane_pids())
    }

    fn caller_in_own_pane(&self) -> Result<bool, MultiplexerError> {
        let Some(terminal) = self.tmux.caller_terminal() else {
            return Ok(false);
        };
        let output = self
            .tmux
            .query(&TmuxScripts::all_panes(TmuxListing::TTY_FORMAT))?;
        if output.has("ringleader-tty") || output.succeeded() {
            return Ok(TmuxListing::new(&output).shows_terminal(terminal));
        }
        if output.reports_no_sessions() {
            return Ok(false);
        }
        Err(TmuxError::failed("list-panes", &output).into())
    }

    fn caller_in_other_tmux(&self) -> bool {
        self.tmux.caller_in_other_tmux()
    }

    fn caller_session(&self) -> Result<Option<String>, MultiplexerError> {
        let Some(pane) = self.tmux.caller_pane() else {
            return Ok(None);
        };
        let lookup = TmuxScripts::pane_value(&pane, "ringleader-caller #{session_name}");
        let output = self.tmux.query(&lookup)?;
        Ok(output
            .first("ringleader-caller")
            .filter(|name| !name.is_empty())
            .map(str::to_string))
    }

    fn observe_all(&self) -> Result<Vec<SessionObservation>, MultiplexerError> {
        let output = self
            .tmux
            .query(&TmuxScripts::all_panes(&TmuxListing::pane_format()))?;
        if output.has("ringleader-pane") || output.succeeded() {
            return Ok(TmuxListing::new(&output).observations());
        }
        if output.reports_no_sessions() {
            return Ok(Vec::new());
        }
        Err(TmuxError::failed("list-panes", &output).into())
    }

    fn attach(&self, target: &AttachTarget) -> Result<(), MultiplexerError> {
        let attach = self.attach_command(target)?;
        self.attach_in_foreground(attach)
    }

    fn switch_client(&self, client: &str, target: &AttachTarget) -> Result<(), MultiplexerError> {
        let resolved = self.resolved(target)?;
        let output = self
            .tmux
            .change(&TmuxScripts::switch_client(client, &resolved))?;
        match output.succeeded() {
            true => Ok(()),
            false => Err(TmuxError::failed("switch-client", &output).into()),
        }
    }

    fn capture_pane(&self, tmux_name: &str) -> Result<String, MultiplexerError> {
        let pane = self.shown_pane(tmux_name)?;
        let output = self.tmux.query(&TmuxScripts::capture(&pane))?;
        if !output.succeeded() && output.stdout().is_empty() {
            return Err(TmuxError::failed("capture-pane", &output).into());
        }
        Ok(output.stdout().to_string())
    }

    fn fit_agent_window(&self, tmux_name: &str, size: SessionSize) -> Result<(), MultiplexerError> {
        let pane = self.shown_pane(tmux_name)?;
        let output = self.tmux.change(&TmuxScripts::fit_window(&pane, size))?;
        if !output.has(TmuxLaunch::DONE_LABEL) {
            log::warn!("{}", TmuxError::failed("resize-window", &output));
            return Err(MultiplexerError::WindowNotFitted {
                tmux_name: tmux_name.to_string(),
            });
        }
        if Self::resized(&output) {
            thread::sleep(Self::AGENT_REDRAW_TIME);
        }
        Ok(())
    }

    fn rename_session(
        &self,
        old: &str,
        new: &str,
        metadata: &SessionMetadata,
    ) -> Result<(), MultiplexerError> {
        let output = self.tmux.change(&TmuxScripts::rename(old, new, metadata))?;
        Self::require_done("rename-session", &output)
    }

    fn respawn_agent(&self, tmux_name: &str, launch: &AgentLaunch) -> Result<(), MultiplexerError> {
        let prepared = self.prepare(launch)?;
        Self::discarding_on_failure(&prepared, self.revive_agent(tmux_name, launch, &prepared))
    }

    fn type_when_ready(&self, tmux_name: &str, lines: &[String]) -> Result<(), MultiplexerError> {
        self.type_into_agent(tmux_name, lines, Self::STARTUP_PATIENCE)
    }

    fn type_if_ready(&self, tmux_name: &str, lines: &[String]) -> Result<(), MultiplexerError> {
        self.type_into_agent(tmux_name, lines, Duration::ZERO)
    }

    fn submit_when_ready(&self, tmux_name: &str, prompt: &str) -> Result<(), MultiplexerError> {
        self.reach_agent(tmux_name, Self::STARTUP_PATIENCE, |input, pane| {
            input.paste_when_ready(pane, prompt)
        })
    }

    fn agent_at_prompt(&self, tmux_name: &str) -> Result<bool, MultiplexerError> {
        let pane = self.required_agent_pane(tmux_name)?;
        let input = TmuxAgentInput::new(&self.tmux, Duration::ZERO);
        Ok(input.is_ready(&pane)?)
    }

    fn ensure_shell_window(
        &self,
        tmux_name: &str,
        worktree: &Path,
        shell: &str,
    ) -> Result<String, MultiplexerError> {
        let listing = self.tmux.query(&TmuxScripts::shell_windows(tmux_name))?;
        if let Some(window) = TmuxListing::new(&listing).shell_window() {
            return Ok(window);
        }
        let sequence = TmuxScripts::shell_window(tmux_name, worktree, shell);
        let output = self.tmux.change(&sequence)?;
        let launched = TmuxLaunch::new(&self.tmux).run(&sequence, output)?;
        Ok(launched.container().to_string())
    }

    fn server_state(&self) -> Result<Option<ServerState>, MultiplexerError> {
        self.private_server().state()
    }

    fn server_config(
        &self,
        key_bindings: &KeyBindings,
    ) -> Result<Option<ServerConfig>, MultiplexerError> {
        self.private_server().config(key_bindings)
    }

    fn socket_lost(&self) -> bool {
        self.private_server().socket_lost()
    }

    fn restore_socket(&self) -> Result<bool, MultiplexerError> {
        self.private_server().restore_socket()
    }

    fn apply_config(&self, key_bindings: &KeyBindings) -> Result<(), MultiplexerError> {
        self.private_server().apply(key_bindings)
    }

    fn version(&self) -> Result<String, MultiplexerError> {
        Ok(self.tmux.version()?)
    }
}

impl From<TmuxError> for MultiplexerError {
    fn from(error: TmuxError) -> Self {
        match error {
            TmuxError::Failed { subcommand, detail } => {
                MultiplexerError::Failed { subcommand, detail }
            }
            TmuxError::TimedOut {
                subcommand,
                seconds,
            } => MultiplexerError::TimedOut {
                subcommand,
                seconds,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::os::unix::net::UnixListener;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::adapters::host::process_environment::ProcessEnvironment;
    use crate::adapters::host::system_host_probe::SystemHostProbe;
    use crate::adapters::tmux::tmux_sequence::TmuxSequence;
    use crate::adapters::{Deadlines, HostProcessTable};
    use crate::application::ports::HostProbe;
    use crate::domain::{
        HotKey, KeyBindings, RepoName, SessionClassifier, SessionRepo, SessionState,
        SessionWorktree,
    };
    use crate::test_support::{DirenvTestHome, TmuxTestServer};

    const HOOKED: &str = "run-shell \"echo hooked; exit 1\"";

    struct MultiplexerFixture {
        server: TmuxTestServer,
    }

    impl MultiplexerFixture {
        const PROMPT_CHECK_LIMIT: Duration = Duration::from_secs(5);

        fn new() -> Self {
            Self {
                server: TmuxTestServer::new(),
            }
        }

        fn with_holder() -> Self {
            Self {
                server: TmuxTestServer::with_session("holder"),
            }
        }

        fn with_recorded_holder() -> Self {
            let fixture = Self::with_holder();
            fixture.server.multiplexer().server_state().unwrap();
            fixture
        }

        fn window_size(&self, target: &str) -> String {
            self.server.text(&[
                "display-message",
                "-p",
                "-t",
                target,
                "#{window_width}x#{window_height}",
            ])
        }

        fn worktree(&self, name: &str) -> PathBuf {
            let worktree = self.server.directory().join("worktrees").join(name);
            fs::create_dir_all(&worktree).unwrap();
            worktree
        }

        fn metadata(&self, name: &str) -> SessionMetadata {
            SessionMetadata::new(
                SessionRepo::new(
                    RepoName::parse("example-project").unwrap(),
                    &self.server.directory().join("example-project"),
                ),
                SessionWorktree::new(&self.worktree(name), &format!("ringleader/{name}"), ""),
                "stand-in",
            )
        }

        fn launch(&self, name: &str) -> AgentLaunch {
            AgentLaunch::new(vec!["stand-in".to_string()], &[], &self.worktree(name))
        }

        fn spec(&self, name: &str) -> SessionSpec {
            SessionSpec::new(
                &format!("ringleader-{name}"),
                self.metadata(name),
                self.launch(name),
            )
        }

        fn create(&self, name: &str) -> Result<PathBuf, MultiplexerError> {
            self.server.multiplexer().create_session(&self.spec(name))
        }

        fn sessions(&self) -> Vec<String> {
            let output = self
                .server
                .tmux(&["list-sessions", "-F", "#{session_name}"]);
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::to_string)
                .collect()
        }

        fn observation(&self, tmux_name: &str) -> SessionObservation {
            self.server
                .multiplexer()
                .observe_all()
                .unwrap()
                .into_iter()
                .find(|observation| observation.tmux_name() == tmux_name)
                .unwrap()
        }

        fn tagged_panes(&self, tmux_name: &str) -> Vec<String> {
            let listing = self.server.text(&[
                "list-panes",
                "-s",
                "-t",
                &format!("={tmux_name}:"),
                "-F",
                "#{pane_id} #{@ringleader_agent}",
            ]);
            listing
                .lines()
                .filter_map(|line| line.strip_suffix(" 1"))
                .map(str::to_string)
                .collect()
        }

        fn hook(&self, name: &str, command: &str) {
            self.server.text(&["set-hook", "-g", name, command]);
        }

        fn created_pane(&self, tmux_name: &str) -> String {
            self.server.pane_of(&format!("={tmux_name}:0.0"))
        }

        fn recorded_shell(&self, tmux_name: &str) -> String {
            let target = format!("={tmux_name}:");
            self.server
                .text(&["show-options", "-t", &target, "-v", "default-command"])
        }

        fn attached_to(&self, target: &AttachTarget) -> Vec<String> {
            let attach = self.server.multiplexer().attach_command(target).unwrap();
            let arguments: Vec<String> = attach
                .get_args()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect();
            let subcommand = arguments
                .iter()
                .position(|argument| argument == "attach-session")
                .unwrap();
            arguments[subcommand..].to_vec()
        }

        fn session_named_by(&self, target: &str) -> String {
            self.server
                .text(&["display-message", "-p", "-t", target, "#{session_name}"])
        }

        fn signal(pid: &str, signal: &str) {
            std::process::Command::new("kill")
                .args([signal, pid])
                .status()
                .unwrap();
        }

        fn on_terminal(&self, terminal: Option<&str>) -> TmuxMultiplexer {
            let base = self.server.environment();
            let environment =
                ProcessEnvironment::new(base.variables().to_vec(), terminal.map(OsString::from));
            TmuxMultiplexer::new(
                self.server.command_with(&environment),
                self.server.stand_in(),
                self.server.process_table(),
            )
        }
    }

    struct DirenvPane {
        fixture: MultiplexerFixture,
        home: DirenvTestHome,
    }

    impl DirenvPane {
        const ENVRC: &'static str = "export EXAMPLE_FROM_ENVRC=loaded\n";
        const IDLE_SECONDS: u64 = 3;
        const NOW: u64 = 0;

        fn started(agent: &str) -> Self {
            let pane = Self {
                fixture: MultiplexerFixture::new(),
                home: DirenvTestHome::new(),
            };
            let worktree = pane.fixture.worktree("fix-auth");
            fs::write(worktree.join(".envrc"), Self::ENVRC).unwrap();
            pane.home.allow_as_the_user(&worktree);
            pane.multiplexer(&worktree, agent)
                .create_session(&pane.fixture.spec("fix-auth"))
                .unwrap();
            pane
        }

        fn multiplexer(&self, worktree: &Path, agent: &str) -> TmuxMultiplexer {
            let worktree = worktree.to_string_lossy().into_owned();
            let prefix = ["direnv", "exec", &worktree, "sh", "-c", agent, "sh"];
            TmuxMultiplexer::new(
                self.fixture.server.command_with(&self.home.environment()),
                self.fixture
                    .server
                    .launcher(prefix.iter().map(OsString::from).collect()),
                self.fixture.server.process_table(),
            )
        }

        fn runs_in_pane(&self, command: &str) -> bool {
            let multiplexer = self.fixture.server.multiplexer();
            let pane = multiplexer.pane_pids("ringleader-fix-auth").unwrap();
            let probe = SystemHostProbe::new(
                &self.home.environment(),
                OsStr::new("tmux"),
                self.fixture.server.process_table(),
            );
            probe
                .session_processes(&pane)
                .iter()
                .any(|process| process.command() == command)
        }

        fn state(&self) -> SessionState {
            let observation = self.fixture.observation("ringleader-fix-auth");
            SessionClassifier::new(Self::IDLE_SECONDS).classify(Some(&observation), Self::NOW)
        }
    }

    struct UsersServer;

    impl UsersServer {
        fn snapshot(server: &TmuxTestServer) -> (String, String) {
            (
                server.text(&["list-keys"]),
                server.text(&["list-sessions", "-F", "#{session_name} #{session_windows}"]),
            )
        }

        fn caller_inside(fixture: &MultiplexerFixture, users: &TmuxTestServer) -> TmuxMultiplexer {
            let pane = users.pane_of("=users-work:");
            let tmux_value = format!("{},{},0", users.socket().display(), users.server_pid());
            let environment = fixture
                .server
                .environment_with(&[("TMUX", &tmux_value), ("TMUX_PANE", &pane)]);
            TmuxMultiplexer::new(
                fixture.server.command_with(&environment),
                fixture.server.stand_in(),
                fixture.server.process_table(),
            )
        }

        fn exercise(fixture: &MultiplexerFixture, multiplexer: &TmuxMultiplexer) {
            multiplexer
                .create_session(&fixture.spec("fix-auth"))
                .unwrap();
            multiplexer.observe_all().unwrap();
            multiplexer.capture_pane("ringleader-fix-auth").unwrap();
            multiplexer
                .ensure_shell_window("ringleader-fix-auth", &fixture.worktree("fix-auth"), "sh")
                .unwrap();
            multiplexer.agent_pane("ringleader-fix-auth").unwrap();
            multiplexer.caller_session().unwrap();
            multiplexer.caller_in_own_pane().unwrap();
            multiplexer.server_state().unwrap();
            multiplexer.version().unwrap();
            multiplexer
                .apply_config(
                    &KeyBindings::default().with_return_key(HotKey::parse("M-x").unwrap()),
                )
                .unwrap();
            let agent = fixture.tagged_panes("ringleader-fix-auth").pop().unwrap();
            fixture
                .server
                .text(&["respawn-pane", "-k", "-t", &agent, "true"]);
            fixture
                .server
                .wait_for(|| fixture.observation("ringleader-fix-auth").pane_dead());
            multiplexer
                .respawn_agent("ringleader-fix-auth", &fixture.launch("fix-auth"))
                .unwrap();
            multiplexer
                .rename_session(
                    "ringleader-fix-auth",
                    "ringleader-renamed",
                    &fixture.metadata("fix-auth"),
                )
                .unwrap();
            multiplexer.kill_session("ringleader-renamed").unwrap();
        }
    }

    #[test]
    fn create_returns_the_socket_of_the_server_it_used() {
        let fixture = MultiplexerFixture::new();

        let socket = fixture.create("fix-auth").unwrap();

        assert_eq!(socket, fixture.server.socket());
    }

    #[test]
    fn a_created_session_carries_its_metadata_and_a_live_agent_pane() {
        let fixture = MultiplexerFixture::new();
        fixture.create("fix-auth").unwrap();

        let observation = fixture.observation("ringleader-fix-auth");

        assert_eq!(observation.metadata(), Some(&fixture.metadata("fix-auth")));
        assert!(!observation.pane_dead());
    }

    #[test]
    fn a_created_session_keeps_its_agent_pane_and_survives_detaching() {
        let fixture = MultiplexerFixture::new();
        fixture.create("fix-auth").unwrap();
        let pane = fixture.tagged_panes("ringleader-fix-auth").pop().unwrap();

        let remain =
            fixture
                .server
                .text(&["show-options", "-p", "-v", "-t", &pane, "remain-on-exit"]);
        let destroy = fixture.server.text(&[
            "show-options",
            "-v",
            "-t",
            "=ringleader-fix-auth:",
            "destroy-unattached",
        ]);

        assert_eq!((remain.as_str(), destroy.as_str()), ("on", "off"));
    }

    #[test]
    fn killing_by_exact_target_spares_a_session_with_a_longer_name() {
        let fixture = MultiplexerFixture::new();
        fixture.create("fix-auth").unwrap();

        fixture
            .server
            .multiplexer()
            .kill_session("ringleader-fix")
            .unwrap();

        assert_eq!(fixture.sessions(), ["ringleader-fix-auth"]);
    }

    #[test]
    fn utf8_metadata_and_tabs_survive_a_c_locale() {
        let fixture = MultiplexerFixture::new();
        let environment = fixture
            .server
            .environment_with(&[("LC_ALL", "C"), ("LANG", "C")]);
        let multiplexer = TmuxMultiplexer::new(
            fixture.server.command_with(&environment),
            fixture.server.stand_in(),
            fixture.server.process_table(),
        );
        let worktree = fixture.server.directory().join("worktrees").join("café-☕");
        fs::create_dir_all(&worktree).unwrap();
        let metadata = SessionMetadata::new(
            SessionRepo::new(RepoName::parse("example-project").unwrap(), &worktree),
            SessionWorktree::new(&worktree, "ringleader/cafe", ""),
            "stand-in",
        );
        let launch = AgentLaunch::new(Vec::new(), &[], &worktree);
        multiplexer
            .create_session(&SessionSpec::new(
                "ringleader-cafe",
                metadata.clone(),
                launch,
            ))
            .unwrap();

        let observations = multiplexer.observe_all().unwrap();

        assert_eq!(observations[0].metadata(), Some(&metadata));
    }

    #[test]
    fn an_absent_socket_file_observes_zero_sessions() {
        let fixture = MultiplexerFixture::new();

        assert!(
            fixture
                .server
                .multiplexer()
                .observe_all()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_stale_socket_observes_zero_sessions() {
        let fixture = MultiplexerFixture::new();
        drop(UnixListener::bind(fixture.server.socket()).unwrap());

        assert!(
            fixture
                .server
                .multiplexer()
                .observe_all()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_live_server_without_sessions_observes_zero_sessions() {
        let fixture = MultiplexerFixture::with_holder();
        fixture
            .server
            .text(&["set-option", "-s", "exit-empty", "off"]);
        fixture.server.text(&["kill-session", "-t", "=holder:"]);

        assert!(
            fixture
                .server
                .multiplexer()
                .observe_all()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_failing_later_command_leaves_no_session() {
        let fixture = MultiplexerFixture::with_holder();
        let tmux = fixture.server.command();
        let sequence = TmuxSequence::new()
            .command([
                "new-session",
                "-d",
                "-s",
                "ringleader-x",
                "-P",
                "-F",
                "ringleader-created #{session_id} #{pane_id}",
                "sleep",
                "30",
            ])
            .command(["set-option", "-t", "=missing:", "@x", "1"])
            .command(["display-message", "-p", "ringleader-done"]);
        let output = tmux.create(&sequence, &[]).unwrap();

        let result = TmuxLaunch::new(&tmux).run(&sequence, output);

        assert!(result.is_err());
        assert_eq!(fixture.sessions(), ["holder"]);
    }

    #[test]
    fn a_hook_that_renames_the_new_session_leaves_no_session() {
        let fixture = MultiplexerFixture::with_holder();
        fixture.hook("after-new-session", "rename-session renamed-by-hook");

        assert!(fixture.create("fix-auth").is_err());
        assert_eq!(fixture.sessions(), ["holder"]);
    }

    #[test]
    fn a_failed_tag_move_leaves_no_session() {
        let fixture = MultiplexerFixture::with_holder();
        fixture.hook(
            "after-new-session",
            "split-window \"sleep 30\" ; kill-pane -t !",
        );

        assert!(fixture.create("fix-auth").is_err());
        assert_eq!(fixture.sessions(), ["holder"]);
    }

    #[test]
    fn a_hook_that_splits_the_window_still_tags_the_launcher_pane() {
        let fixture = MultiplexerFixture::with_holder();
        fixture.hook("after-new-session", "split-window \"sleep 30\"");

        fixture.create("fix-auth").unwrap();

        let launcher_pane = fixture.created_pane("ringleader-fix-auth");
        assert_eq!(fixture.tagged_panes("ringleader-fix-auth"), [launcher_pane]);
    }

    #[test]
    fn a_name_taken_after_the_availability_check_leaves_that_session_running() {
        let fixture = MultiplexerFixture::with_holder();
        fixture.server.start_session("ringleader-fix-auth");

        assert!(fixture.create("fix-auth").is_err());
        assert_eq!(fixture.sessions(), ["holder", "ringleader-fix-auth"]);
        assert_eq!(fixture.server.handoff_files(), 0);
    }

    #[test]
    fn a_failed_respawn_leaves_no_handoff_file() {
        let fixture = MultiplexerFixture::with_holder();
        fixture.server.start_session("ringleader-fix-auth");
        fixture.hook("after-new-window", "rename-session renamed-by-hook");

        let result = fixture
            .server
            .multiplexer()
            .respawn_agent("ringleader-fix-auth", &fixture.launch("fix-auth"));

        assert!(result.is_err());
        assert_eq!(fixture.server.handoff_files(), 0);
    }

    #[test]
    fn the_launcher_receives_the_handoff_path() {
        let fixture = MultiplexerFixture::new();
        let multiplexer = TmuxMultiplexer::new(
            fixture.server.command(),
            fixture.server.launcher(
                [
                    "sh",
                    "-c",
                    "printf '%s %s\\n' \"$1\" \"$2\"; sleep 30",
                    "--",
                ]
                .map(OsString::from)
                .to_vec(),
            ),
            fixture.server.process_table(),
        );
        multiplexer
            .create_session(&fixture.spec("fix-auth"))
            .unwrap();

        let shown = fixture.server.wait_for(|| {
            multiplexer
                .capture_pane("ringleader-fix-auth")
                .is_ok_and(|text| text.contains("--handoff") && text.contains("handoff-"))
        });

        assert!(shown);
    }

    #[test]
    fn noisy_failing_hooks_never_undo_a_creation_shell_window_or_rename() {
        let fixture = MultiplexerFixture::with_holder();
        for hook in [
            "after-new-window",
            "after-set-option",
            "after-rename-session",
            "after-display-message",
        ] {
            fixture.hook(hook, HOOKED);
        }
        fixture.hook(
            "after-new-session",
            &format!("split-window \"sleep 30\" ; {HOOKED}"),
        );
        let multiplexer = fixture.server.multiplexer();

        fixture.create("fix-auth").unwrap();
        let launcher_pane = fixture.created_pane("ringleader-fix-auth");
        let window = multiplexer
            .ensure_shell_window("ringleader-fix-auth", &fixture.worktree("fix-auth"), "sh")
            .unwrap();
        multiplexer
            .rename_session(
                "ringleader-fix-auth",
                "ringleader-renamed",
                &fixture.metadata("fix-auth"),
            )
            .unwrap();

        assert_eq!(fixture.tagged_panes("ringleader-renamed"), [launcher_pane]);
        assert!(window.starts_with('@'));
        assert_eq!(fixture.server.handoff_files(), 1);
        assert!(
            fixture
                .sessions()
                .contains(&"ringleader-renamed".to_string())
        );
    }

    #[test]
    fn a_server_without_a_detach_binding_rolls_the_creation_back() {
        let fixture = MultiplexerFixture {
            server: TmuxTestServer::with_config("set -g prefix None\n"),
        };
        fixture.server.start_session("holder");

        let error = fixture.create("fix-auth").unwrap_err();

        assert!(matches!(error, MultiplexerError::NoReturnBinding { .. }));
        assert!(
            error
                .to_string()
                .contains(&fixture.server.config().display().to_string())
        );
        assert_eq!(fixture.sessions(), ["holder"]);
    }

    #[test]
    fn a_creation_writes_the_generated_config() {
        let fixture = MultiplexerFixture::new();
        fs::remove_file(fixture.server.config()).unwrap();

        fixture.create("fix-auth").unwrap();

        assert!(
            fs::read_to_string(fixture.server.config())
                .unwrap()
                .contains("detach-client")
        );
    }

    #[test]
    fn a_caller_whose_terminal_is_a_pane_of_the_server_is_in_its_own_pane() {
        let fixture = MultiplexerFixture::with_holder();
        let tty = fixture
            .server
            .text(&["display-message", "-p", "-t", "=holder:", "#{pane_tty}"]);

        let inside = fixture
            .on_terminal(Some(&tty))
            .caller_in_own_pane()
            .unwrap();

        assert!(inside);
    }

    #[test]
    fn the_terminal_an_exited_agent_pane_freed_is_not_its_own_pane() {
        let fixture = MultiplexerFixture::new();
        fixture.create("fix-auth").unwrap();
        let agent = fixture.tagged_panes("ringleader-fix-auth").pop().unwrap();
        fixture
            .server
            .text(&["respawn-pane", "-k", "-t", &agent, "true"]);
        let exited = fixture
            .server
            .wait_for(|| fixture.observation("ringleader-fix-auth").pane_dead());
        let tty = fixture
            .server
            .text(&["display-message", "-p", "-t", &agent, "#{pane_tty}"]);

        let inside = fixture
            .on_terminal(Some(&tty))
            .caller_in_own_pane()
            .unwrap();

        assert!(exited);
        assert!(!inside);
    }

    #[test]
    fn a_caller_on_another_terminal_with_an_inherited_tmux_is_not_in_its_own_pane() {
        let fixture = MultiplexerFixture::with_holder();
        let multiplexer = fixture.on_terminal(Some("/dev/pts/4000001"));

        assert!(!multiplexer.caller_in_own_pane().unwrap());
    }

    #[test]
    fn a_caller_without_a_terminal_or_a_server_is_not_in_its_own_pane() {
        let without_server = MultiplexerFixture::new();
        let without_terminal = MultiplexerFixture::with_holder();

        let on_terminal = without_server.on_terminal(Some("/dev/pts/4000001"));

        assert!(!on_terminal.caller_in_own_pane().unwrap());
        assert!(
            !without_terminal
                .on_terminal(None)
                .caller_in_own_pane()
                .unwrap()
        );
    }

    #[test]
    fn the_caller_session_ignores_a_pane_of_another_tmux_server() {
        let fixture = MultiplexerFixture::with_holder();
        let users_server = TmuxTestServer::with_session("users-work");
        let pane = fixture.server.pane_of("=holder:");
        let tmux_value = format!("{},1,0", users_server.socket().display());
        let environment = fixture
            .server
            .environment_with(&[("TMUX", &tmux_value), ("TMUX_PANE", &pane)]);
        let multiplexer = TmuxMultiplexer::new(
            fixture.server.command_with(&environment),
            fixture.server.stand_in(),
            fixture.server.process_table(),
        );

        assert_eq!(multiplexer.caller_session().unwrap(), None);
    }

    #[test]
    fn working_on_the_private_server_leaves_the_users_tmux_server_unchanged() {
        let fixture = MultiplexerFixture::new();
        let users_server = TmuxTestServer::with_config("bind-key -T root C-q send-keys x\n");
        users_server.start_session("users-work");
        let before = UsersServer::snapshot(&users_server);
        let multiplexer = UsersServer::caller_inside(&fixture, &users_server);

        UsersServer::exercise(&fixture, &multiplexer);

        assert_eq!(UsersServer::snapshot(&users_server), before);
    }

    #[test]
    fn a_tmux_variable_naming_another_server_is_a_caller_in_other_tmux() {
        let fixture = MultiplexerFixture::new();
        let other = format!(
            "{},1,0",
            fixture.server.directory().join("users-tmux").display()
        );
        let own = format!("{},1,0", fixture.server.socket().display());

        let callers = [Some(other.as_str()), Some(own.as_str()), None].map(|tmux| {
            let extra: Vec<(&str, &str)> = tmux.map(|value| ("TMUX", value)).into_iter().collect();
            let environment = fixture.server.environment_with(&extra);
            TmuxMultiplexer::new(
                fixture.server.command_with(&environment),
                fixture.server.stand_in(),
                fixture.server.process_table(),
            )
            .caller_in_other_tmux()
        });

        assert_eq!(callers, [true, false, false]);
    }

    #[test]
    fn rename_moves_the_metadata_to_the_new_name() {
        let fixture = MultiplexerFixture::new();
        fixture.create("a").unwrap();
        let renamed = fixture.metadata("a").with_branch("ringleader/b");

        fixture
            .server
            .multiplexer()
            .rename_session("ringleader-a", "ringleader-b", &renamed)
            .unwrap();

        assert_eq!(
            fixture.observation("ringleader-b").metadata(),
            Some(&renamed)
        );
    }

    #[test]
    fn a_rename_onto_an_existing_name_changes_nothing() {
        let fixture = MultiplexerFixture::new();
        fixture.create("a").unwrap();
        fixture.create("b").unwrap();

        let result = fixture.server.multiplexer().rename_session(
            "ringleader-a",
            "ringleader-b",
            &fixture.metadata("a").with_branch("ringleader/b"),
        );

        assert!(result.is_err());
        assert_eq!(
            fixture.observation("ringleader-a").metadata(),
            Some(&fixture.metadata("a"))
        );
        assert_eq!(
            fixture.observation("ringleader-b").metadata(),
            Some(&fixture.metadata("b"))
        );
    }

    #[test]
    fn pane_pids_lists_every_live_pane_and_skips_a_dead_agent_pane() {
        let fixture = MultiplexerFixture::new();
        fixture.create("fix-auth").unwrap();
        let agent = fixture.tagged_panes("ringleader-fix-auth").pop().unwrap();
        fixture
            .server
            .text(&["new-window", "-t", "=ringleader-fix-auth:", "sleep", "30"]);
        fixture
            .server
            .text(&["split-window", "-t", "=ringleader-fix-auth:", "sleep", "30"]);
        fixture
            .server
            .text(&["respawn-pane", "-k", "-t", &agent, "true"]);
        let dead = fixture.server.wait_for(|| {
            fixture
                .server
                .text(&["display-message", "-p", "-t", &agent, "#{pane_dead}"])
                == "1"
        });

        let pids = fixture
            .server
            .multiplexer()
            .pane_pids("ringleader-fix-auth")
            .unwrap();

        assert!(dead);
        assert_eq!(pids.len(), 2);
    }

    #[test]
    fn respawn_revives_the_tagged_pane_while_a_shell_window_is_current() {
        let fixture = MultiplexerFixture::new();
        fixture.create("fix-auth").unwrap();
        let multiplexer = fixture.server.multiplexer();
        let agent = fixture.tagged_panes("ringleader-fix-auth").pop().unwrap();
        fixture
            .server
            .text(&["respawn-pane", "-k", "-t", &agent, "true"]);
        fixture
            .server
            .wait_for(|| fixture.observation("ringleader-fix-auth").pane_dead());
        multiplexer
            .ensure_shell_window("ringleader-fix-auth", &fixture.worktree("fix-auth"), "sh")
            .unwrap();

        multiplexer
            .respawn_agent("ringleader-fix-auth", &fixture.launch("fix-auth"))
            .unwrap();

        assert!(!fixture.observation("ringleader-fix-auth").pane_dead());
        assert_eq!(fixture.tagged_panes("ringleader-fix-auth"), [agent]);
    }

    #[test]
    fn a_recreated_agent_window_starts_in_the_worktree() {
        let fixture = MultiplexerFixture::with_holder();
        fixture.server.start_session("ringleader-fix-auth");
        let worktree = fixture.worktree("fix-auth");

        fixture
            .server
            .multiplexer()
            .respawn_agent("ringleader-fix-auth", &fixture.launch("fix-auth"))
            .unwrap();

        let agent = fixture.tagged_panes("ringleader-fix-auth").pop().unwrap();
        let expected = worktree.to_string_lossy().into_owned();
        assert!(fixture.server.wait_for(|| {
            fixture.server.text(&[
                "display-message",
                "-p",
                "-t",
                &agent,
                "#{pane_current_path}",
            ]) == expected
        }));
    }

    #[test]
    fn a_hook_that_renames_the_session_undoes_a_new_agent_window() {
        let fixture = MultiplexerFixture::with_holder();
        fixture.server.start_session("ringleader-fix-auth");
        fixture.hook("after-new-window", "rename-session renamed-by-hook");

        let result = fixture
            .server
            .multiplexer()
            .respawn_agent("ringleader-fix-auth", &fixture.launch("fix-auth"));

        let windows = fixture.server.text(&[
            "list-windows",
            "-t",
            "=renamed-by-hook:",
            "-F",
            "#{window_id}",
        ]);
        assert!(result.is_err());
        assert_eq!(windows.lines().count(), 1);
    }

    #[test]
    fn the_shell_window_is_reused_once_created() {
        let fixture = MultiplexerFixture::new();
        fixture.create("fix-auth").unwrap();
        let multiplexer = fixture.server.multiplexer();
        let worktree = fixture.worktree("fix-auth");

        let first = multiplexer
            .ensure_shell_window("ringleader-fix-auth", &worktree, "sh")
            .unwrap();
        fixture
            .server
            .text(&["rename-window", "-t", &first, "renamed"]);
        let second = multiplexer
            .ensure_shell_window("ringleader-fix-auth", &worktree, "sh")
            .unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn the_attach_targets_the_agent_pane_id() {
        let fixture = MultiplexerFixture::new();
        fixture.create("fix-auth").unwrap();
        let agent = fixture.tagged_panes("ringleader-fix-auth").pop().unwrap();

        let attach = fixture.attached_to(&AttachTarget::Agent("ringleader-fix-auth".to_string()));

        assert_eq!(attach, ["attach-session", "-t", agent.as_str()]);
    }

    #[test]
    fn the_attach_to_a_session_without_an_agent_pane_targets_its_exact_name() {
        let fixture = MultiplexerFixture::with_holder();

        let attach = fixture.attached_to(&AttachTarget::Agent("holder".to_string()));

        assert_eq!(attach, ["attach-session", "-t", "=holder:"]);
    }

    #[test]
    fn a_client_attached_to_the_agent_pane_shows_it_while_the_shell_window_is_current() {
        let fixture = MultiplexerFixture::new();
        fixture.create("fix-auth").unwrap();
        let agent = fixture.tagged_panes("ringleader-fix-auth").pop().unwrap();
        fixture
            .server
            .multiplexer()
            .ensure_shell_window("ringleader-fix-auth", &fixture.worktree("fix-auth"), "sh")
            .unwrap();

        let client = fixture.server.control_client(&agent);

        assert_eq!(fixture.server.client_pane(&client), agent);
    }

    #[test]
    fn switching_a_client_shows_the_agent_pane_of_the_other_session() {
        let fixture = MultiplexerFixture::new();
        fixture.create("fix-auth").unwrap();
        fixture.create("dark-mode").unwrap();
        let agent = fixture.tagged_panes("ringleader-dark-mode").pop().unwrap();
        let client = fixture.server.control_client("=ringleader-fix-auth:");
        let name = fixture.server.client_name(&client);

        fixture
            .server
            .multiplexer()
            .switch_client(
                &name,
                &AttachTarget::Agent("ringleader-dark-mode".to_string()),
            )
            .unwrap();

        assert_eq!(fixture.server.client_pane(&client), agent);
    }

    #[test]
    fn switching_a_client_to_a_shell_window_shows_that_window() {
        let fixture = MultiplexerFixture::new();
        fixture.create("fix-auth").unwrap();
        fixture.create("dark-mode").unwrap();
        let shell = fixture
            .server
            .multiplexer()
            .ensure_shell_window("ringleader-dark-mode", &fixture.worktree("dark-mode"), "sh")
            .unwrap();
        let client = fixture.server.control_client("=ringleader-fix-auth:");
        let name = fixture.server.client_name(&client);

        fixture
            .server
            .multiplexer()
            .switch_client(&name, &AttachTarget::Shell(shell.clone()))
            .unwrap();

        let window = fixture.server.text(&["list-clients", "-F", "#{window_id}"]);
        assert_eq!(window, shell);
    }

    #[test]
    fn switching_a_client_that_is_gone_fails() {
        let fixture = MultiplexerFixture::with_holder();

        let switched = fixture
            .server
            .multiplexer()
            .switch_client("client-0", &AttachTarget::Agent("holder".to_string()));

        assert!(switched.is_err());
    }

    #[test]
    fn capture_reads_the_agent_pane() {
        let fixture = MultiplexerFixture::with_holder();
        let multiplexer = TmuxMultiplexer::new(
            fixture.server.command(),
            fixture.server.launcher(
                ["sh", "-c", "printf 'agent says hi\\n'; sleep 30", "--"]
                    .map(OsString::from)
                    .to_vec(),
            ),
            fixture.server.process_table(),
        );
        multiplexer
            .create_session(&fixture.spec("fix-auth"))
            .unwrap();

        let captured = fixture.server.wait_for(|| {
            multiplexer
                .capture_pane("ringleader-fix-auth")
                .is_ok_and(|text| text.contains("agent says hi"))
        });

        assert!(captured);
    }

    #[test]
    fn capturing_a_missing_session_reports_the_failed_pane_lookup() {
        let fixture = MultiplexerFixture::with_holder();

        let error = fixture
            .server
            .multiplexer()
            .capture_pane("ringleader-missing")
            .unwrap_err();

        assert!(matches!(error, MultiplexerError::Failed { .. }));
    }

    #[test]
    fn fitting_the_agent_window_resizes_it_to_the_given_size() {
        let fixture = MultiplexerFixture::new();
        fixture.server.managed_session("fix-auth");

        fixture
            .server
            .multiplexer()
            .fit_agent_window("ringleader-fix-auth", SessionSize::new(60, 20))
            .unwrap();

        assert_eq!(fixture.window_size("=ringleader-fix-auth:"), "60x20");
    }

    #[test]
    fn an_attached_client_keeps_its_own_size_when_the_window_is_fitted() {
        let fixture = MultiplexerFixture::new();
        fixture.server.managed_session("fix-auth");
        let client = fixture.server.control_client("=ringleader-fix-auth:");
        let client_name = fixture.server.client_name(&client);
        fixture
            .server
            .text(&["refresh-client", "-t", &client_name, "-C", "90x30"]);

        fixture
            .server
            .multiplexer()
            .fit_agent_window("ringleader-fix-auth", SessionSize::new(60, 20))
            .unwrap();

        assert_eq!(fixture.window_size("=ringleader-fix-auth:"), "90x30");
    }

    #[test]
    fn capture_reads_a_session_without_an_agent_pane() {
        let fixture = MultiplexerFixture::new();
        fixture.server.text(&[
            "new-session",
            "-d",
            "-s",
            "handmade",
            "sh",
            "-c",
            "printf 'made by hand\\n'; sleep 30",
        ]);
        let multiplexer = fixture.server.multiplexer();

        let captured = fixture.server.wait_for(|| {
            multiplexer
                .capture_pane("handmade")
                .is_ok_and(|text| text.contains("made by hand"))
        });

        assert!(captured, "{:?}", multiplexer.capture_pane("handmade"));
    }

    #[test]
    fn a_session_named_like_another_sessions_id_is_attached_by_its_own_id() {
        let fixture = MultiplexerFixture::new();
        fixture.create("fix-auth").unwrap();
        fixture.server.start_session("$0");

        let attach = fixture.attached_to(&AttachTarget::Agent("$0".to_string()));

        let target = attach.last().unwrap();
        assert_eq!(fixture.session_named_by(target), "$0");
    }

    #[test]
    fn a_session_named_like_another_sessions_id_is_captured_from_its_own_pane() {
        let fixture = MultiplexerFixture::with_holder();
        fixture.server.text(&[
            "new-session",
            "-d",
            "-s",
            "$0",
            "sh",
            "-c",
            "printf 'named like an id\\n'; sleep 30",
        ]);
        let multiplexer = fixture.server.multiplexer();

        let captured = fixture.server.wait_for(|| {
            multiplexer
                .capture_pane("$0")
                .is_ok_and(|text| text.contains("named like an id"))
        });

        assert!(captured, "{:?}", multiplexer.capture_pane("$0"));
    }

    #[test]
    fn creating_after_the_socket_was_removed_reuses_the_recorded_server() {
        let fixture = MultiplexerFixture::with_recorded_holder();
        let first_server = fixture.server.server_pid();
        fs::remove_file(fixture.server.socket()).unwrap();

        fixture.create("fix-auth").unwrap();

        assert_eq!(fixture.server.server_pid(), first_server);
        assert!(
            fixture
                .server
                .text(&["list-sessions", "-F", "#{session_name}"])
                .contains("holder")
        );
    }

    #[test]
    fn a_stopped_server_that_cannot_restore_its_socket_refuses_the_creation() {
        let fixture = MultiplexerFixture::with_recorded_holder();
        let first_server = fixture.server.server_pid();
        MultiplexerFixture::signal(&first_server, "-STOP");
        fs::remove_file(fixture.server.socket()).unwrap();

        let result = fixture.create("fix-auth");

        MultiplexerFixture::signal(&first_server, "-CONT");
        let socket_back = fixture.server.wait_for(|| fixture.server.socket().exists());
        assert!(
            matches!(result, Err(MultiplexerError::SocketNotRestored { .. })),
            "{result:?}"
        );
        assert!(socket_back);
        assert_eq!(fixture.server.server_pid(), first_server);
        assert_eq!(fixture.sessions(), ["holder"]);
    }

    #[test]
    fn typed_input_reaches_the_agent_pane_at_its_prompt() {
        let fixture = MultiplexerFixture::with_holder();
        let multiplexer = TmuxMultiplexer::new(
            fixture.server.command(),
            fixture.server.launcher(
                ["sh", "-c", "printf '> '; exec cat", "--"]
                    .map(OsString::from)
                    .to_vec(),
            ),
            fixture.server.process_table(),
        );
        multiplexer
            .create_session(&fixture.spec("fix-auth"))
            .unwrap();

        multiplexer
            .type_when_ready("ringleader-fix-auth", &["hello fix-auth".to_string()])
            .unwrap();

        let typed = fixture.server.wait_for(|| {
            multiplexer
                .capture_pane("ringleader-fix-auth")
                .is_ok_and(|text| text.contains("hello fix-auth"))
        });
        assert!(typed);
    }

    #[test]
    fn a_submitted_prompt_reaches_the_agent_pane_at_its_prompt() {
        let fixture = MultiplexerFixture::with_holder();
        let multiplexer = TmuxMultiplexer::new(
            fixture.server.command(),
            fixture.server.launcher(
                ["sh", "-c", "printf '> '; exec cat", "--"]
                    .map(OsString::from)
                    .to_vec(),
            ),
            fixture.server.process_table(),
        );
        multiplexer
            .create_session(&fixture.spec("fix-auth"))
            .unwrap();

        multiplexer
            .submit_when_ready("ringleader-fix-auth", "review fix-auth")
            .unwrap();

        let submitted = fixture.server.wait_for(|| {
            multiplexer
                .capture_pane("ringleader-fix-auth")
                .is_ok_and(|text| text.contains("review fix-auth"))
        });
        assert!(submitted);
    }

    #[test]
    fn an_agent_away_from_its_prompt_is_reported_by_session_name() {
        let fixture = MultiplexerFixture::with_holder();
        fixture.create("fix-auth").unwrap();

        let typed = fixture
            .server
            .multiplexer()
            .type_if_ready("ringleader-fix-auth", &["/rename fix-auth".to_string()]);

        assert_eq!(
            typed,
            Err(MultiplexerError::AgentNotReady {
                tmux_name: "ringleader-fix-auth".to_string()
            })
        );
    }

    #[test]
    fn an_agent_showing_a_bare_prompt_marker_is_at_its_prompt() {
        let fixture = MultiplexerFixture::with_holder();
        let multiplexer = TmuxMultiplexer::new(
            fixture.server.command(),
            fixture.server.launcher(
                ["sh", "-c", "printf '> '; exec cat", "--"]
                    .map(OsString::from)
                    .to_vec(),
            ),
            fixture.server.process_table(),
        );
        multiplexer
            .create_session(&fixture.spec("fix-auth"))
            .unwrap();

        let at_prompt = fixture.server.wait_for(|| {
            multiplexer
                .agent_at_prompt("ringleader-fix-auth")
                .unwrap_or(false)
        });

        assert!(at_prompt);
    }

    #[test]
    fn an_agent_with_no_prompt_marker_is_not_at_its_prompt() {
        let fixture = MultiplexerFixture::with_holder();
        fixture.create("fix-auth").unwrap();

        let at_prompt = fixture
            .server
            .multiplexer()
            .agent_at_prompt("ringleader-fix-auth");

        assert_eq!(at_prompt, Ok(false));
    }

    #[test]
    fn typing_after_a_rename_checks_the_prompt_without_waiting() {
        let fixture = MultiplexerFixture::with_holder();
        fixture.create("fix-auth").unwrap();
        let started = Instant::now();

        let typed = fixture
            .server
            .multiplexer()
            .type_if_ready("ringleader-fix-auth", &["/rename fix-auth".to_string()]);

        assert!(typed.is_err());
        assert!(started.elapsed() < MultiplexerFixture::PROMPT_CHECK_LIMIT);
    }

    #[test]
    fn the_caller_session_is_the_one_holding_the_pane_variable() {
        let fixture = MultiplexerFixture::with_holder();
        let pane = fixture.server.pane_of("=holder:");
        let tmux_value = format!("{},1,0", fixture.server.socket().display());
        let environment = fixture
            .server
            .environment_with(&[("TMUX", &tmux_value), ("TMUX_PANE", &pane)]);
        let multiplexer = TmuxMultiplexer::new(
            fixture.server.command_with(&environment),
            fixture.server.launcher(Vec::new()),
            fixture.server.process_table(),
        );

        assert_eq!(
            multiplexer.caller_session().unwrap(),
            Some("holder".to_string())
        );
    }

    #[test]
    fn a_fixture_secret_never_reaches_the_server_it_starts() {
        let fixture = MultiplexerFixture::new();
        let secret = "fixture-secret-value";
        let environment = fixture
            .server
            .environment_with(&[("EXAMPLE_API_KEY", secret)]);
        let names = vec!["EXAMPLE_API_KEY".to_string()];
        let multiplexer = TmuxMultiplexer::new(
            fixture
                .server
                .command_with(&environment)
                .removing_env_names(&names),
            fixture.server.stand_in(),
            fixture.server.process_table(),
        );
        let spec = fixture.spec("fix-auth").removing_env_names(&names);

        multiplexer.create_session(&spec).unwrap();

        let pid = fixture.server.server_pid().parse().unwrap();
        let global = fixture.server.text(&["show-environment", "-g"]);
        let environment = HostProcessTable::environment(pid);
        let server = fixture
            .server
            .process_table()
            .read(pid)
            .expect("the server process is readable");
        assert!(!global.contains(secret));
        assert!(
            environment
                .iter()
                .any(|variable| variable.starts_with("PATH="))
        );
        assert!(
            environment
                .iter()
                .all(|variable| !variable.contains(secret))
        );
        assert!(!server.info().command().contains(secret));
    }

    #[test]
    fn a_created_session_records_the_creators_shell_whatever_the_agent_sets() {
        let fixture = MultiplexerFixture::new();
        let environment = fixture
            .server
            .environment_with(&[("SHELL", "/bin/example-shell")]);
        let agent_env = vec!["SHELL".to_string()];
        let multiplexer = TmuxMultiplexer::new(
            fixture
                .server
                .command_with(&environment)
                .removing_env_names(&agent_env),
            fixture.server.stand_in(),
            fixture.server.process_table(),
        );
        let agent_shell = [("SHELL".to_string(), "/bin/agent-shell".to_string())];
        let launch = fixture
            .launch("fix-auth")
            .with_env_assignments(&agent_shell);
        let spec = SessionSpec::new("ringleader-fix-auth", fixture.metadata("fix-auth"), launch)
            .removing_env_names(&agent_env);

        multiplexer.create_session(&spec).unwrap();

        assert_eq!(
            fixture.recorded_shell("ringleader-fix-auth"),
            "/bin/example-shell"
        );
    }

    #[test]
    fn a_stopped_server_times_out_the_observation() {
        let fixture = MultiplexerFixture::with_holder();
        let deadline = Duration::from_secs(1);
        let multiplexer = TmuxMultiplexer::new(
            fixture.server.command_with_deadlines(
                &fixture.server.environment(),
                Deadlines::uniform(deadline),
            ),
            fixture.server.launcher(Vec::new()),
            fixture.server.process_table(),
        );
        let pid = fixture.server.server_pid();
        MultiplexerFixture::signal(&pid, "-STOP");
        let started = Instant::now();

        let result = multiplexer.observe_all();

        MultiplexerFixture::signal(&pid, "-CONT");
        assert!(matches!(result, Err(MultiplexerError::TimedOut { .. })));
        assert!(started.elapsed() < deadline + Duration::from_secs(3));
    }

    #[test]
    fn a_blocking_hook_times_out_the_creation_and_a_kill_cleans_up() {
        let fixture = MultiplexerFixture::with_holder();
        fixture.hook("after-new-session", "run-shell 'sleep 3'");
        let impatient = TmuxMultiplexer::new(
            fixture.server.command_with_deadlines(
                &fixture.server.environment(),
                Deadlines::uniform(Duration::from_secs(1)),
            ),
            fixture.server.stand_in(),
            fixture.server.process_table(),
        );

        let error = impatient
            .create_session(&fixture.spec("fix-auth"))
            .unwrap_err();
        fixture
            .server
            .multiplexer()
            .kill_session("ringleader-fix-auth")
            .unwrap();

        assert!(error.is_timeout());
        assert_eq!(fixture.sessions(), ["holder"]);
    }

    #[test]
    fn an_agent_started_through_direnv_runs_among_the_processes_of_its_pane() {
        let pane = DirenvPane::started("exec sleep 31");

        assert!(
            pane.fixture
                .server
                .wait_for(|| pane.runs_in_pane("sleep 31"))
        );
    }

    #[test]
    fn an_agent_started_through_direnv_that_exits_leaves_an_exited_session() {
        let pane = DirenvPane::started("echo \"$EXAMPLE_FROM_ENVRC\" > agent-ran");
        let fixture = &pane.fixture;

        assert!(
            fixture
                .server
                .wait_for(|| { fixture.observation("ringleader-fix-auth").pane_dead() })
        );
        assert_eq!(pane.state(), SessionState::Exited);
        let ran = fs::read_to_string(fixture.worktree("fix-auth").join("agent-ran")).unwrap();
        assert_eq!(ran, "loaded\n");
    }
}
