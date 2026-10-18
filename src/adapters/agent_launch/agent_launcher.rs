use std::ffi::OsString;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

use thiserror::Error;

use super::agent_handoff::AgentHandoff;
use super::handoff_contents::{EnvrcStep, HandoffContents};
use super::launcher_command::{LauncherCommand, PreparedLaunch};
use crate::adapters::host::process_environment::ProcessEnvironment;
use crate::domain::Agent;

pub trait ExecSeam {
    fn exec(&mut self, command: Command) -> io::Error;
}

pub struct ProcessExec;

impl ExecSeam for ProcessExec {
    fn exec(&mut self, mut command: Command) -> io::Error {
        command.exec()
    }
}

pub struct AgentLauncher {
    environment: ProcessEnvironment,
    seam: Box<dyn ExecSeam>,
    reentry: LauncherCommand,
}

impl AgentLauncher {
    const PANE_VARIABLES: [&'static str; 3] = ["TERM", "TMUX", "TMUX_PANE"];
    const ENVRC_LOADER: &'static str = "direnv";
    const ENVRC_LOADER_EXEC: &'static str = "exec";

    pub fn new(
        environment: &ProcessEnvironment,
        seam: Box<dyn ExecSeam>,
        reentry: LauncherCommand,
    ) -> Self {
        Self {
            environment: environment.clone(),
            seam,
            reentry,
        }
    }

    pub fn launch(&mut self, handoff: &Path) -> LaunchError {
        match Self::checked_contents(handoff) {
            Ok(contents) => self.exec_step(&contents),
            Err(error) => error,
        }
    }

    fn checked_contents(handoff: &Path) -> Result<HandoffContents, LaunchError> {
        let contents = AgentHandoff::take(handoff).map_err(|error| LaunchError::Handoff {
            path: handoff.display().to_string(),
            detail: error.to_string(),
        })?;
        Self::check(&contents)?;
        Ok(contents)
    }

    fn exec_step(&mut self, contents: &HandoffContents) -> LaunchError {
        match contents.envrc() {
            EnvrcStep::Skip => self.exec(self.agent_command(contents, contents.inherited())),
            EnvrcStep::Load => self.exec_through_envrc(contents),
            EnvrcStep::Loaded => self.exec_inside_envrc(contents),
        }
    }

    fn check(contents: &HandoffContents) -> Result<(), LaunchError> {
        let worktree = contents.worktree();
        if !worktree.is_dir() {
            return Err(LaunchError::MissingWorktree {
                path: worktree.display().to_string(),
            });
        }
        if contents.argv().is_empty() {
            return Err(LaunchError::EmptyCommand);
        }
        Ok(())
    }

    fn exec_through_envrc(&mut self, contents: &HandoffContents) -> LaunchError {
        let prepared = match self.reentry.prepare_relay(&contents.inside_envrc()) {
            Ok(prepared) => prepared,
            Err(error) => {
                return LaunchError::EnvrcHandoff {
                    detail: error.to_string(),
                };
            }
        };
        let loader = Self::envrc_loader(contents, &prepared);
        let error = self.exec(self.in_pane(loader, contents, contents.inherited()));
        prepared.discard();
        error
    }

    fn envrc_loader(contents: &HandoffContents, reentry: &PreparedLaunch) -> Command {
        let mut loader = Command::new(Self::ENVRC_LOADER);
        loader
            .arg(Self::ENVRC_LOADER_EXEC)
            .arg(contents.worktree())
            .args(reentry.words());
        loader
    }

    fn exec_inside_envrc(&mut self, contents: &HandoffContents) -> LaunchError {
        let loaded = self.environment.variables().to_vec();
        self.exec(self.agent_command(contents, loaded))
    }

    fn agent_command(
        &self,
        contents: &HandoffContents,
        mut environment: Vec<(OsString, OsString)>,
    ) -> Command {
        let argv = contents.argv();
        let mut command = Command::new(&argv[0]);
        command.args(&argv[1..]);
        environment.extend(contents.explicit());
        self.in_pane(command, contents, environment)
    }

    fn in_pane(
        &self,
        mut command: Command,
        contents: &HandoffContents,
        environment: Vec<(OsString, OsString)>,
    ) -> Command {
        command
            .env_clear()
            .envs(environment)
            .envs(self.pane_variables())
            .envs(Agent::TRUE_COLOR_VARIABLES)
            .current_dir(contents.worktree());
        if let Some(terminal) = self.environment.terminal() {
            command.env("GPG_TTY", terminal);
        }
        command
    }

    fn exec(&mut self, command: Command) -> LaunchError {
        LaunchError::Exec {
            detail: self.seam.exec(command).to_string(),
        }
    }

    fn pane_variables(&self) -> Vec<(OsString, OsString)> {
        Self::PANE_VARIABLES
            .iter()
            .filter_map(|name| {
                let value = self.environment.get(name)?;
                Some((OsString::from(name), value.to_os_string()))
            })
            .collect()
    }
}

#[derive(Debug, Error)]
pub enum LaunchError {
    #[error("cannot read the handoff file {path}: {detail}")]
    Handoff { path: String, detail: String },
    #[error("the worktree {path} does not exist, run ringleader doctor")]
    MissingWorktree { path: String },
    #[error("the handoff file names no agent command")]
    EmptyCommand,
    #[error("cannot write the handoff file the agent takes inside its .envrc: {detail}")]
    EnvrcHandoff { detail: String },
    #[error("cannot start the agent: {detail}")]
    Exec { detail: String },
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsStr;
    use std::path::PathBuf;
    use std::process::Stdio;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::application::ports::AgentLaunch;
    use crate::test_support::{DirenvTestHome, InheritedPath, ProcessMarker};

    #[derive(Clone, Default)]
    struct CapturingExec {
        captured: Arc<Mutex<Option<Captured>>>,
    }

    struct Captured {
        output: String,
        directory: Option<PathBuf>,
    }

    impl CapturingExec {
        fn environment(&self) -> BTreeMap<String, String> {
            let captured = self.captured.lock().unwrap();
            captured
                .as_ref()
                .unwrap()
                .output
                .lines()
                .filter_map(|line| line.split_once('='))
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect()
        }

        fn directory(&self) -> Option<PathBuf> {
            self.captured
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .directory
                .clone()
        }

        fn ran(&self) -> bool {
            self.captured.lock().unwrap().is_some()
        }
    }

    impl ExecSeam for CapturingExec {
        fn exec(&mut self, mut command: Command) -> io::Error {
            let directory = command.get_current_dir().map(Path::to_path_buf);
            let output = command.stdout(Stdio::piped()).output();
            *self.captured.lock().unwrap() = Some(Captured {
                output: output
                    .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
                    .unwrap_or_default(),
                directory,
            });
            io::Error::other("captured instead of exec")
        }
    }

    #[derive(Clone, Default)]
    struct RecordingExec {
        recorded: Arc<Mutex<Vec<Recorded>>>,
    }

    struct Recorded {
        words: Vec<OsString>,
        environment: BTreeMap<OsString, OsString>,
    }

    impl RecordingExec {
        fn words(&self) -> Vec<OsString> {
            self.recorded.lock().unwrap()[0].words.clone()
        }

        fn variable(&self, name: &str) -> Option<OsString> {
            self.recorded.lock().unwrap()[0]
                .environment
                .get(OsStr::new(name))
                .cloned()
        }
    }

    impl ExecSeam for RecordingExec {
        fn exec(&mut self, command: Command) -> io::Error {
            let mut words = vec![command.get_program().to_os_string()];
            words.extend(command.get_args().map(OsStr::to_os_string));
            let environment = command
                .get_envs()
                .filter_map(|(name, value)| Some((name.to_os_string(), value?.to_os_string())))
                .collect();
            self.recorded
                .lock()
                .unwrap()
                .push(Recorded { words, environment });
            io::Error::other("recorded instead of exec")
        }
    }

    struct LauncherFixture {
        directory: tempfile::TempDir,
    }

    impl LauncherFixture {
        const REENTRY: [&'static str; 2] = ["ringleader", "__exec-agent"];
        const SCRATCH: &'static str = "/scratch/fix-auth";

        fn new() -> Self {
            let fixture = Self {
                directory: tempfile::tempdir().unwrap(),
            };
            std::fs::create_dir(fixture.worktree()).unwrap();
            fixture
        }

        fn worktree(&self) -> PathBuf {
            self.directory.path().join("fix-auth")
        }

        fn handoff_directory(&self) -> PathBuf {
            self.directory.path().join("handoff")
        }

        fn env_program() -> String {
            Self::program_on_path("env")
        }

        fn program_on_path(name: &str) -> String {
            InheritedPath::program(name)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        }

        fn words(words: &[&str]) -> Vec<OsString> {
            words.iter().map(OsString::from).collect()
        }

        fn creating_snapshot() -> ProcessEnvironment {
            ProcessEnvironment::new(
                vec![
                    ("PATH".into(), "/creator/bin".into()),
                    ("HOME".into(), "/home/example".into()),
                    ("EXAMPLE_API_KEY".into(), "fixture-secret".into()),
                    ("UNLISTED".into(), "never".into()),
                    ("GPG_TTY".into(), "/dev/pts/77".into()),
                ],
                None,
            )
        }

        fn pane_snapshot(terminal: Option<&str>) -> ProcessEnvironment {
            ProcessEnvironment::new(
                vec![
                    ("PATH".into(), "/inherited/bin".into()),
                    ("TERM".into(), "tmux-256color".into()),
                    ("TMUX".into(), "/tmp/tmux-1000/default,1,0".into()),
                    ("TMUX_PANE".into(), "%3".into()),
                    ("HOME".into(), "/inherited/home".into()),
                ],
                terminal.map(OsString::from),
            )
        }

        fn envrc_snapshot() -> ProcessEnvironment {
            ProcessEnvironment::new(
                vec![
                    ("PATH".into(), "/devshell/bin:/creator/bin".into()),
                    ("TMPDIR".into(), "/envrc/tmp".into()),
                    ("SHELL".into(), "/envrc/bin/bash".into()),
                    ("EXAMPLE_ACCOUNT_DIR".into(), "/envrc/accounts".into()),
                    ("EXAMPLE_API_KEY".into(), "from-envrc".into()),
                    ("EXAMPLE_FROM_ENVRC".into(), "loaded".into()),
                    ("COLORTERM".into(), "from-envrc".into()),
                ],
                None,
            )
        }

        fn agent_launch(worktree: &Path) -> AgentLaunch {
            AgentLaunch::new(
                vec![Self::env_program()],
                &["EXAMPLE_API_KEY".to_string()],
                worktree,
            )
        }

        fn envrc_launch(worktree: &Path) -> AgentLaunch {
            let assignments = [(
                "EXAMPLE_ACCOUNT_DIR".to_string(),
                "/accounts/example".to_string(),
            )];
            Self::agent_launch(worktree)
                .with_env_assignments(&assignments)
                .with_temp_directory(Path::new(Self::SCRATCH))
                .loading_envrc()
        }

        fn handoff_from(&self, creating: &ProcessEnvironment, launch: &AgentLaunch) -> PathBuf {
            let handoff = AgentHandoff::new(
                &self.handoff_directory(),
                creating,
                ProcessMarker::new().table(),
            );
            handoff.write(launch).unwrap()
        }

        fn handoff(&self, launch: &AgentLaunch) -> PathBuf {
            self.handoff_from(&Self::creating_snapshot(), launch)
        }

        fn launcher(
            pane: &ProcessEnvironment,
            seam: Box<dyn ExecSeam>,
            handoff: &Path,
            reentry: &[&str],
        ) -> AgentLauncher {
            let reentry = LauncherCommand::new(
                Self::words(reentry),
                AgentHandoff::beside(handoff, pane, ProcessMarker::new().table()),
            );
            AgentLauncher::new(pane, seam, reentry)
        }

        fn launch(
            &self,
            terminal: Option<&str>,
            worktree: &Path,
        ) -> (CapturingExec, LaunchError, PathBuf) {
            self.run(terminal, &Self::agent_launch(worktree))
        }

        fn run(
            &self,
            terminal: Option<&str>,
            launch: &AgentLaunch,
        ) -> (CapturingExec, LaunchError, PathBuf) {
            let seam = CapturingExec::default();
            let handoff = self.handoff(launch);
            let pane = Self::pane_snapshot(terminal);
            let mut launcher =
                Self::launcher(&pane, Box::new(seam.clone()), &handoff, &Self::REENTRY);
            let result = launcher.launch(&handoff);
            (seam, result, handoff)
        }

        fn record(&self, launch: &AgentLaunch) -> RecordingExec {
            let seam = RecordingExec::default();
            let handoff = self.handoff(launch);
            let pane = Self::pane_snapshot(None);
            Self::launcher(&pane, Box::new(seam.clone()), &handoff, &Self::REENTRY)
                .launch(&handoff);
            seam
        }

        fn inside_envrc(&self) -> CapturingExec {
            self.inside_envrc_from(&Self::creating_snapshot())
        }

        fn inside_envrc_from(&self, creating: &ProcessEnvironment) -> CapturingExec {
            let first = self.handoff_from(creating, &Self::envrc_launch(&self.worktree()));
            let contents = AgentHandoff::take(&first).unwrap().inside_envrc();
            let envrc = Self::envrc_snapshot();
            let handoff = AgentHandoff::beside(&first, &envrc, ProcessMarker::new().table())
                .write_relay(&contents)
                .unwrap();
            let seam = CapturingExec::default();
            Self::launcher(&envrc, Box::new(seam.clone()), &handoff, &Self::REENTRY)
                .launch(&handoff);
            seam
        }
    }

    #[test]
    fn the_agent_gets_exactly_the_handed_values_the_pane_variables_and_true_color() {
        let fixture = LauncherFixture::new();

        let (seam, _, _) = fixture.launch(Some("/dev/pts/9"), &fixture.worktree());

        let expected: BTreeMap<String, String> = [
            ("PATH", "/creator/bin"),
            ("HOME", "/home/example"),
            ("EXAMPLE_API_KEY", "fixture-secret"),
            ("TERM", "tmux-256color"),
            ("TMUX", "/tmp/tmux-1000/default,1,0"),
            ("TMUX_PANE", "%3"),
            ("COLORTERM", "truecolor"),
            ("CLAUDE_CODE_TMUX_TRUECOLOR", "1"),
            ("GPG_TTY", "/dev/pts/9"),
        ]
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
        assert_eq!(seam.environment(), expected);
    }

    #[test]
    fn opencode_is_told_the_terminal_renders_true_color() {
        let fixture = LauncherFixture::new();

        let (seam, _, _) = fixture.launch(None, &fixture.worktree());

        assert_eq!(seam.environment()["COLORTERM"], "truecolor");
    }

    #[test]
    fn claude_code_is_told_it_may_use_true_color_inside_tmux() {
        let fixture = LauncherFixture::new();

        let (seam, _, _) = fixture.launch(None, &fixture.worktree());

        assert_eq!(seam.environment()["CLAUDE_CODE_TMUX_TRUECOLOR"], "1");
    }

    #[test]
    fn the_agent_gets_its_arguments() {
        let fixture = LauncherFixture::new();
        let argv = [
            LauncherFixture::program_on_path("sh"),
            "-c".to_string(),
            "echo ARGUMENTS=$*".to_string(),
            "sh".to_string(),
            "--name".to_string(),
            "fix-auth".to_string(),
        ];
        let launch = AgentLaunch::new(argv.to_vec(), &[], &fixture.worktree());

        let (seam, _, _) = fixture.run(None, &launch);

        assert_eq!(seam.environment()["ARGUMENTS"], "--name fix-auth");
    }

    #[test]
    fn a_set_value_reaches_the_agent_in_place_of_the_inherited_one() {
        let fixture = LauncherFixture::new();
        let launch = LauncherFixture::agent_launch(&fixture.worktree())
            .with_env_assignments(&[("HOME".to_string(), "/home/example-account".to_string())]);

        let (seam, _, _) = fixture.run(None, &launch);

        assert_eq!(seam.environment()["HOME"], "/home/example-account");
    }

    #[test]
    fn gpg_tty_is_absent_without_a_terminal() {
        let fixture = LauncherFixture::new();

        let (seam, _, _) = fixture.launch(None, &fixture.worktree());

        assert!(!seam.environment().contains_key("GPG_TTY"));
    }

    #[test]
    fn an_inherited_value_for_an_allowlisted_name_never_reaches_the_agent() {
        let fixture = LauncherFixture::new();

        let (seam, _, _) = fixture.launch(None, &fixture.worktree());

        assert_eq!(seam.environment()["HOME"], "/home/example");
    }

    #[test]
    fn the_agent_starts_in_the_worktree() {
        let fixture = LauncherFixture::new();

        let (seam, _, _) = fixture.launch(None, &fixture.worktree());

        assert_eq!(seam.directory(), Some(fixture.worktree()));
    }

    #[test]
    fn the_handoff_file_is_deleted_before_exec() {
        let fixture = LauncherFixture::new();

        let (_, _, handoff) = fixture.launch(None, &fixture.worktree());

        assert!(!handoff.exists());
    }

    #[test]
    fn a_missing_worktree_fails_before_exec() {
        let fixture = LauncherFixture::new();
        let missing = fixture.directory.path().join("gone");

        let (seam, result, _) = fixture.launch(None, &missing);

        assert!(matches!(result, LaunchError::MissingWorktree { .. }));
        assert!(!seam.ran());
    }

    #[test]
    fn an_unreadable_handoff_fails_before_exec() {
        let fixture = LauncherFixture::new();
        let missing = fixture.handoff_directory().join("handoff-gone.json");
        let seam = CapturingExec::default();
        let pane = LauncherFixture::pane_snapshot(None);

        let result = LauncherFixture::launcher(
            &pane,
            Box::new(seam.clone()),
            &missing,
            &LauncherFixture::REENTRY,
        )
        .launch(&missing);

        assert!(matches!(result, LaunchError::Handoff { .. }));
        assert!(!seam.ran());
    }

    #[test]
    fn a_handoff_without_a_command_fails_before_exec() {
        let fixture = LauncherFixture::new();
        let launch = AgentLaunch::new(Vec::new(), &[], &fixture.worktree());

        let (seam, result, _) = fixture.run(None, &launch);

        assert!(matches!(result, LaunchError::EmptyCommand));
        assert!(!seam.ran());
    }

    #[test]
    fn a_second_handoff_that_cannot_be_written_fails_before_direnv() {
        let fixture = LauncherFixture::new();
        let handoff = fixture.handoff(&LauncherFixture::envrc_launch(&fixture.worktree()));
        let blocked = fixture.directory.path().join("blocked");
        std::fs::write(&blocked, "a file where the handoff directory goes").unwrap();
        let pane = LauncherFixture::pane_snapshot(None);
        let reentry = LauncherCommand::new(
            LauncherFixture::words(&LauncherFixture::REENTRY),
            AgentHandoff::new(
                &blocked.join("handoff"),
                &pane,
                ProcessMarker::new().table(),
            ),
        );
        let seam = RecordingExec::default();

        let result = AgentLauncher::new(&pane, Box::new(seam.clone()), reentry).launch(&handoff);

        assert!(matches!(result, LaunchError::EnvrcHandoff { .. }));
        assert!(seam.recorded.lock().unwrap().is_empty());
    }

    #[test]
    fn a_launch_through_the_envrc_hands_the_launcher_to_direnv_exec_in_the_worktree() {
        let fixture = LauncherFixture::new();

        let seam = fixture.record(&LauncherFixture::envrc_launch(&fixture.worktree()));

        let words = seam.words();
        let worktree = fixture.worktree().into_os_string();
        assert_eq!(
            words[..5],
            [
                OsString::from("direnv"),
                OsString::from("exec"),
                worktree,
                OsString::from("ringleader"),
                OsString::from("__exec-agent")
            ]
        );
        assert_eq!(words[5], "--handoff");
        assert_eq!(
            Path::new(&words[6]).parent(),
            Some(fixture.handoff_directory().as_path())
        );
    }

    #[test]
    fn direnv_loads_the_envrc_with_the_home_ringleader_started_with() {
        let fixture = LauncherFixture::new();
        let assignments = [("HOME".to_string(), "/home/example-account".to_string())];
        let launch =
            LauncherFixture::envrc_launch(&fixture.worktree()).with_env_assignments(&assignments);

        let seam = fixture.record(&launch);

        assert_eq!(seam.variable("HOME"), Some(OsString::from("/home/example")));
    }

    #[test]
    fn a_direnv_exec_that_fails_leaves_no_handoff_file() {
        let fixture = LauncherFixture::new();

        fixture.record(&LauncherFixture::envrc_launch(&fixture.worktree()));

        let left = std::fs::read_dir(fixture.handoff_directory())
            .unwrap()
            .count();
        assert_eq!(left, 0);
    }

    #[test]
    fn the_scratch_tmpdir_wins_over_the_envrc_one() {
        let seam = LauncherFixture::new().inside_envrc();

        assert_eq!(seam.environment()["TMPDIR"], LauncherFixture::SCRATCH);
    }

    #[test]
    fn the_vouched_colorterm_wins_over_the_envrc_one() {
        let seam = LauncherFixture::new().inside_envrc();

        assert_eq!(seam.environment()["COLORTERM"], "truecolor");
    }

    #[test]
    fn a_set_value_wins_over_the_envrc_one() {
        let seam = LauncherFixture::new().inside_envrc();

        assert_eq!(
            seam.environment()["EXAMPLE_ACCOUNT_DIR"],
            "/accounts/example"
        );
    }

    #[test]
    fn a_configured_value_from_ringleader_wins_over_the_envrc_one() {
        let seam = LauncherFixture::new().inside_envrc();

        assert_eq!(seam.environment()["EXAMPLE_API_KEY"], "fixture-secret");
    }

    #[test]
    fn the_shell_ringleader_started_with_wins_over_the_envrc_one() {
        let creating =
            ProcessEnvironment::new(vec![("SHELL".into(), "/bin/example-shell".into())], None);

        let seam = LauncherFixture::new().inside_envrc_from(&creating);

        assert_eq!(seam.environment()["SHELL"], "/bin/example-shell");
    }

    #[test]
    fn the_agent_inside_the_envrc_keeps_the_envrc_path_and_exports() {
        let seam = LauncherFixture::new().inside_envrc();

        let environment = seam.environment();
        assert_eq!(environment["PATH"], "/devshell/bin:/creator/bin");
        assert_eq!(environment["EXAMPLE_FROM_ENVRC"], "loaded");
    }

    #[test]
    fn direnv_exec_runs_the_launcher_inside_an_envrc_allowed_under_the_same_home() {
        let fixture = LauncherFixture::new();
        let home = DirenvTestHome::new();
        let worktree = home.project_with_envrc(
            "fix-auth",
            "export EXAMPLE_FROM_ENVRC=loaded\nPATH_add devshell-bin\n",
        );
        home.allow_as_the_user(&worktree);
        let handoff = fixture.handoff_from(
            &home.environment(),
            &LauncherFixture::agent_launch(&worktree).loading_envrc(),
        );
        let seam = CapturingExec::default();
        let pane = LauncherFixture::pane_snapshot(None);

        LauncherFixture::launcher(
            &pane,
            Box::new(seam.clone()),
            &handoff,
            &["sh", "-c", "env", "sh"],
        )
        .launch(&handoff);

        let environment = seam.environment();
        assert_eq!(environment["EXAMPLE_FROM_ENVRC"], "loaded");
        let devshell = worktree.join("devshell-bin").display().to_string();
        assert!(
            environment["PATH"].starts_with(&devshell),
            "{}",
            environment["PATH"]
        );
    }
}
