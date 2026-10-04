use std::fs::OpenOptions;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use super::agent_launcher::{LauncherCommand, PreparedLaunch};
use super::tmux_command::TmuxCommand;
use super::tmux_error::TmuxError;
use super::tmux_launch::{Launched, TmuxLaunch};
use super::tmux_listing::TmuxListing;
use super::tmux_output::TmuxOutput;
use super::tmux_scripts::TmuxScripts;
use crate::application::ports::{
    AgentLaunch, AttachTarget, Multiplexer, MultiplexerError, SessionSpec,
};
use crate::domain::{SessionMetadata, SessionObservation};

pub struct TmuxMultiplexer {
    tmux: TmuxCommand,
    launcher: LauncherCommand,
    inside_tmux: bool,
}

impl TmuxMultiplexer {
    const EXIT_UNATTACHED_ON: &'static str = "1";
    const MISSING_SESSION: &'static str = "can't find session";
    const BELL: &'static [u8] = b"\x07";

    pub fn new(tmux: TmuxCommand, launcher: LauncherCommand, inside_tmux: bool) -> Self {
        Self {
            tmux,
            launcher,
            inside_tmux,
        }
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
        let sequence = TmuxScripts::create_session(spec, prepared.words());
        let output = self.tmux.create(&sequence, spec.removed_env_names())?;
        let launched = TmuxLaunch::new(&self.tmux).run(&sequence, output)?;
        self.finish_creation(&launched)
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
        let [current_pane, exit_unattached, socket] = values.as_slice() else {
            launch.roll_back(launched);
            return Err(MultiplexerError::Failed {
                subcommand: "new-session".to_string(),
                detail: "tmux printed an incomplete confirmation".to_string(),
            });
        };
        if *exit_unattached == Self::EXIT_UNATTACHED_ON {
            launch.roll_back(launched);
            return Err(MultiplexerError::ExitUnattached);
        }
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

    fn agent_pane(&self, tmux_name: &str) -> Result<Option<String>, MultiplexerError> {
        let lookup = TmuxScripts::session_panes(tmux_name, TmuxListing::AGENT_FORMAT);
        let output = self.tmux.query(&lookup)?;
        if !output.succeeded() && !output.has("ringleader-agent") {
            return Err(TmuxError::failed("list-panes", &output).into());
        }
        Ok(TmuxListing::new(&output).agent_pane())
    }

    fn attach_in_foreground(&self, target: &str) -> Result<(), MultiplexerError> {
        let attach = TmuxScripts::attach_session(target);
        let child =
            self.tmux
                .foreground(&attach)
                .spawn()
                .map_err(|error| MultiplexerError::Failed {
                    subcommand: "attach-session".to_string(),
                    detail: error.to_string(),
                })?;
        let Ok(pid) = libc::pid_t::try_from(child.id()) else {
            return Ok(());
        };
        let mut status: libc::c_int = 0;
        // SAFETY: waitpid and kill only take the attach child's own pid and this process's
        // pid; status is a local the kernel fills, and the child is reaped exactly once here.
        let waited = unsafe {
            loop {
                if libc::waitpid(pid, &mut status, libc::WUNTRACED) == -1 {
                    if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    break -1;
                }
                if !libc::WIFSTOPPED(status) {
                    break 0;
                }
                libc::kill(std::process::id() as libc::pid_t, libc::SIGSTOP);
                libc::kill(pid, libc::SIGCONT);
            }
        };
        if waited == 0 && libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0 {
            return Ok(());
        }
        Err(MultiplexerError::Failed {
            subcommand: "attach-session".to_string(),
            detail: format!("tmux attach-session ended with wait status {status}"),
        })
    }

    fn require_done(subcommand: &str, output: &TmuxOutput) -> Result<(), MultiplexerError> {
        if output.has(TmuxLaunch::DONE_LABEL) {
            return Ok(());
        }
        Err(TmuxError::failed(subcommand, output).into())
    }

    fn write_bell(tty: &Path) {
        let written = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
            .open(tty)
            .and_then(|mut terminal| terminal.write_all(Self::BELL));
        if let Err(error) = written {
            log::debug!("skipped the bell for {}: {error}", tty.display());
        }
    }
}

impl Multiplexer for TmuxMultiplexer {
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
        let list = TmuxScripts::session_panes(tmux_name, TmuxListing::PANE_PID_FORMAT);
        let output = self.tmux.query(&list)?;
        if !output.succeeded() && !output.has("ringleader-pane-pid") {
            return Err(TmuxError::failed("list-panes", &output).into());
        }
        Ok(TmuxListing::new(&output).live_pane_pids())
    }

    fn has_session_on(
        &self,
        socket: &Path,
        tmux_name: &str,
        worktree: &Path,
    ) -> Result<bool, MultiplexerError> {
        let list = TmuxScripts::all_panes(TmuxListing::CLAIM_FORMAT);
        let output = self.tmux.on_socket(socket).query(&list)?;
        if output.has("ringleader-claim") {
            return Ok(TmuxListing::new(&output).holds_claim(tmux_name, worktree));
        }
        if output.succeeded() || output.reports_no_sessions() {
            return Ok(false);
        }
        Err(TmuxError::failed("list-panes", &output).into())
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
            .query(&TmuxScripts::all_panes(TmuxListing::PANE_FORMAT))?;
        if output.has("ringleader-pane") || output.succeeded() {
            return Ok(TmuxListing::new(&output).observations());
        }
        if output.reports_no_sessions() {
            return Ok(Vec::new());
        }
        Err(TmuxError::failed("list-panes", &output).into())
    }

    fn attach(&self, target: &AttachTarget) -> Result<(), MultiplexerError> {
        let resolved = match target {
            AttachTarget::Agent(tmux_name) => self
                .agent_pane(tmux_name)?
                .unwrap_or_else(|| TmuxScripts::exact(tmux_name)),
            AttachTarget::Shell(window) => window.clone(),
        };
        if !self.inside_tmux {
            return self.attach_in_foreground(&resolved);
        }
        let output = self.tmux.change(&TmuxScripts::switch_client(&resolved))?;
        Self::require_done("switch-client", &output)
    }

    fn capture_pane(&self, tmux_name: &str) -> Result<String, MultiplexerError> {
        let pane = self
            .agent_pane(tmux_name)?
            .ok_or_else(|| MultiplexerError::NoAgentPane {
                tmux_name: tmux_name.to_string(),
            })?;
        let output = self.tmux.query(&TmuxScripts::capture(&pane))?;
        if !output.succeeded() && output.stdout().is_empty() {
            return Err(TmuxError::failed("capture-pane", &output).into());
        }
        Ok(output.stdout().to_string())
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

    fn ring_bell(&self) -> Result<(), MultiplexerError> {
        let output = self.tmux.query(&TmuxScripts::clients())?;
        if !output.succeeded() && !output.has("ringleader-client") && !output.reports_no_server() {
            return Err(TmuxError::failed("list-clients", &output).into());
        }
        for tty in TmuxListing::new(&output).bell_ttys() {
            Self::write_bell(&tty);
        }
        Ok(())
    }

    fn server_version(&self) -> Result<Option<String>, MultiplexerError> {
        let output = self
            .tmux
            .query(&TmuxScripts::server_value("ringleader-version #{version}"))?;
        Ok(output.first("ringleader-version").map(str::to_string))
    }

    fn server_exit_unattached(&self) -> Result<Option<bool>, MultiplexerError> {
        let query = TmuxScripts::server_value("ringleader-exit-unattached #{exit-unattached}");
        let output = self.tmux.query(&query)?;
        Ok(output
            .first("ringleader-exit-unattached")
            .map(|value| value == Self::EXIT_UNATTACHED_ON))
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
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::net::UnixListener;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::adapters::Deadlines;
    use crate::adapters::tmux_sequence::TmuxSequence;
    use crate::domain::{RepoName, SessionRepo, SessionWorktree};
    use crate::test_support::TmuxTestServer;

    const HOOKED: &str = "run-shell \"echo hooked; exit 1\"";

    struct MultiplexerFixture {
        server: TmuxTestServer,
    }

    impl MultiplexerFixture {
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
            false,
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
            false,
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
    fn a_server_with_exit_unattached_on_refuses_the_creation() {
        let fixture = MultiplexerFixture::new();
        let config = fixture.server.directory().join("exit-unattached.conf");
        fs::write(&config, "set-option -g exit-unattached on\n").unwrap();
        let multiplexer = TmuxMultiplexer::new(
            fixture.server.command().with_config_file(&config),
            fixture.server.stand_in(),
            false,
        );

        let error = multiplexer
            .create_session(&fixture.spec("fix-auth"))
            .unwrap_err();

        assert_eq!(error, MultiplexerError::ExitUnattached);
        assert!(error.to_string().contains("exit-unattached"));
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
    fn a_session_renamed_by_hand_is_still_live_on_its_socket() {
        let fixture = MultiplexerFixture::new();
        fixture.create("fix-auth").unwrap();
        fixture
            .server
            .text(&["rename-session", "-t", "=ringleader-fix-auth:", "my-work"]);

        let live = fixture
            .server
            .multiplexer()
            .has_session_on(
                fixture.server.socket(),
                "ringleader-fix-auth",
                &fixture.worktree("fix-auth"),
            )
            .unwrap();

        assert!(live);
    }

    #[test]
    fn an_absent_server_holds_no_session() {
        let fixture = MultiplexerFixture::new();

        let live = fixture
            .server
            .multiplexer()
            .has_session_on(
                fixture.server.socket(),
                "ringleader-fix-auth",
                Path::new("/nowhere"),
            )
            .unwrap();

        assert!(!live);
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
    fn attach_after_opening_the_shell_lands_on_the_agent_pane() {
        let fixture = MultiplexerFixture::new();
        fixture.create("fix-auth").unwrap();
        let agent = fixture.tagged_panes("ringleader-fix-auth").pop().unwrap();
        let multiplexer = fixture.server.inside_multiplexer();
        let window = multiplexer
            .ensure_shell_window("ringleader-fix-auth", &fixture.worktree("fix-auth"), "sh")
            .unwrap();
        let client = fixture.server.control_client(&window);

        multiplexer
            .attach(&AttachTarget::Agent("ringleader-fix-auth".to_string()))
            .unwrap();

        assert!(
            fixture
                .server
                .wait_for(|| fixture.server.client_pane(&client) == agent)
        );
    }

    #[test]
    fn opening_the_shell_after_attach_lands_on_the_shell_window() {
        let fixture = MultiplexerFixture::new();
        fixture.create("fix-auth").unwrap();
        let agent = fixture.tagged_panes("ringleader-fix-auth").pop().unwrap();
        let multiplexer = fixture.server.inside_multiplexer();
        let client = fixture.server.control_client(&agent);
        let window = multiplexer
            .ensure_shell_window("ringleader-fix-auth", &fixture.worktree("fix-auth"), "sh")
            .unwrap();
        let shell_pane = fixture.server.pane_of(&window);

        multiplexer.attach(&AttachTarget::Shell(window)).unwrap();

        assert!(
            fixture
                .server
                .wait_for(|| fixture.server.client_pane(&client) == shell_pane)
        );
    }

    #[test]
    fn the_outside_attach_targets_the_agent_pane_id() {
        let argv = TmuxScripts::attach_session("%3").argv();

        assert_eq!(argv, ["attach-session", "-t", "%3"]);
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
    fn capture_reads_the_agent_pane() {
        let fixture = MultiplexerFixture::with_holder();
        let multiplexer = TmuxMultiplexer::new(
            fixture.server.command(),
            fixture.server.launcher(
                ["sh", "-c", "printf 'agent says hi\\n'; sleep 30", "--"]
                    .map(OsString::from)
                    .to_vec(),
            ),
            false,
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
    fn the_caller_session_is_the_one_holding_the_pane_variable() {
        let fixture = MultiplexerFixture::with_holder();
        let pane = fixture.server.pane_of("=holder:");
        let environment = fixture.server.environment_with(&[("TMUX_PANE", &pane)]);
        let multiplexer = TmuxMultiplexer::new(
            fixture.server.command_with(&environment),
            fixture.server.launcher(Vec::new()),
            false,
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
            false,
        );
        let spec = fixture.spec("fix-auth").removing_env_names(&names);

        multiplexer.create_session(&spec).unwrap();

        let pid = fixture.server.server_pid();
        let global = fixture.server.text(&["show-environment", "-g"]);
        let environ = fs::read(format!("/proc/{pid}/environ")).unwrap();
        let cmdline = fs::read(format!("/proc/{pid}/cmdline")).unwrap();
        assert!(!global.contains(secret));
        assert!(!String::from_utf8_lossy(&environ).contains(secret));
        assert!(!String::from_utf8_lossy(&cmdline).contains(secret));
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
            false,
        );
        let pid = fixture.server.server_pid();
        std::process::Command::new("kill")
            .args(["-STOP", &pid])
            .status()
            .unwrap();
        let started = Instant::now();

        let result = multiplexer.observe_all();

        std::process::Command::new("kill")
            .args(["-CONT", &pid])
            .status()
            .unwrap();
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
            false,
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
    fn the_bell_reaches_a_server_whose_only_client_is_in_control_mode() {
        let fixture = MultiplexerFixture::with_holder();
        let _client = fixture.server.control_client("=holder:");

        assert!(fixture.server.multiplexer().ring_bell().is_ok());
    }

    #[test]
    fn a_bell_for_a_tty_that_would_block_returns_at_once() {
        let directory = tempfile::tempdir().unwrap();
        let fifo = directory.path().join("unread-tty");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(made.success());
        let started = Instant::now();

        TmuxMultiplexer::write_bell(&fifo);

        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
