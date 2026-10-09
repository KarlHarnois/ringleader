use std::ffi::OsString;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

use thiserror::Error;

use super::agent_handoff::{AgentHandoff, HandoffContents};
use super::process_environment::ProcessEnvironment;
use crate::application::ports::AgentLaunch;

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
}

impl AgentLauncher {
    const PANE_VARIABLES: [&'static str; 4] = ["TERM", "COLORTERM", "TMUX", "TMUX_PANE"];

    pub fn new(environment: &ProcessEnvironment, seam: Box<dyn ExecSeam>) -> Self {
        Self {
            environment: environment.clone(),
            seam,
        }
    }

    pub fn launch(&mut self, handoff: &Path) -> LaunchError {
        let contents = match AgentHandoff::take(handoff) {
            Ok(contents) => contents,
            Err(error) => {
                return LaunchError::Handoff {
                    path: handoff.display().to_string(),
                    detail: error.to_string(),
                };
            }
        };
        match self.prepared(&contents) {
            Ok(command) => LaunchError::Exec {
                detail: self.seam.exec(command).to_string(),
            },
            Err(error) => error,
        }
    }

    fn prepared(&self, contents: &HandoffContents) -> Result<Command, LaunchError> {
        let worktree = contents.worktree();
        if !worktree.is_dir() {
            return Err(LaunchError::MissingWorktree {
                path: worktree.display().to_string(),
            });
        }
        let argv = contents.argv();
        let Some((program, arguments)) = argv.split_first() else {
            return Err(LaunchError::EmptyCommand);
        };
        let mut command = Command::new(program);
        command
            .args(arguments)
            .env_clear()
            .envs(contents.environment())
            .envs(self.pane_variables())
            .current_dir(&worktree);
        if let Some(terminal) = self.environment.terminal() {
            command.env("GPG_TTY", terminal);
        }
        Ok(command)
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

pub struct LauncherCommand {
    prefix: Vec<OsString>,
    handoff: AgentHandoff,
}

impl LauncherCommand {
    const HANDOFF_FLAG: &'static str = "--handoff";

    pub fn new(prefix: Vec<OsString>, handoff: AgentHandoff) -> Self {
        Self { prefix, handoff }
    }

    pub fn prepare(&self, launch: &AgentLaunch) -> io::Result<PreparedLaunch> {
        let handoff = self.handoff.write(launch)?;
        let mut words = self.prefix.clone();
        words.push(OsString::from(Self::HANDOFF_FLAG));
        words.push(handoff.clone().into_os_string());
        Ok(PreparedLaunch { words, handoff })
    }
}

pub struct PreparedLaunch {
    words: Vec<OsString>,
    handoff: std::path::PathBuf,
}

impl PreparedLaunch {
    pub fn words(&self) -> &[OsString] {
        &self.words
    }

    pub fn discard(&self) {
        AgentHandoff::delete(&self.handoff);
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
    #[error("cannot start the agent: {detail}")]
    Exec { detail: String },
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::process::Stdio;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::application::ports::AgentLaunch;

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

    struct LauncherFixture {
        directory: tempfile::TempDir,
    }

    impl LauncherFixture {
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

        fn env_program() -> String {
            let path = std::env::var_os("PATH").unwrap_or_default();
            std::env::split_paths(&path)
                .map(|directory| directory.join("env"))
                .find(|candidate| candidate.is_file())
                .unwrap()
                .to_string_lossy()
                .into_owned()
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

        fn agent_launch(worktree: &Path) -> AgentLaunch {
            AgentLaunch::new(
                vec![Self::env_program()],
                &["EXAMPLE_API_KEY".to_string()],
                worktree,
            )
        }

        fn handoff(&self, launch: &AgentLaunch) -> PathBuf {
            let handoff = AgentHandoff::new(
                &self.directory.path().join("handoff"),
                &Self::creating_snapshot(),
            );
            handoff.write(launch).unwrap()
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
            let mut launcher =
                AgentLauncher::new(&Self::pane_snapshot(terminal), Box::new(seam.clone()));
            let result = launcher.launch(&handoff);
            (seam, result, handoff)
        }
    }

    #[test]
    fn the_agent_gets_exactly_the_baseline_the_configured_names_and_the_pane_variables() {
        let fixture = LauncherFixture::new();

        let (seam, _, _) = fixture.launch(Some("/dev/pts/9"), &fixture.worktree());

        let expected: BTreeMap<String, String> = [
            ("PATH", "/creator/bin"),
            ("HOME", "/home/example"),
            ("EXAMPLE_API_KEY", "fixture-secret"),
            ("TERM", "tmux-256color"),
            ("TMUX", "/tmp/tmux-1000/default,1,0"),
            ("TMUX_PANE", "%3"),
            ("GPG_TTY", "/dev/pts/9"),
        ]
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
        assert_eq!(seam.environment(), expected);
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
}
