use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

use super::bounded_command::{BoundedCommand, Call, CallKind, CommandError};
use super::caller_process::CallerProcess;
use super::process_environment::ProcessEnvironment;
use super::tmux_output::TmuxOutput;
use super::tmux_sequence::TmuxSequence;

#[derive(Debug, Clone)]
pub struct TmuxCommand {
    program: OsString,
    socket: Option<PathBuf>,
    config: Option<PathBuf>,
    environment: ProcessEnvironment,
    bounded: BoundedCommand,
}

impl TmuxCommand {
    pub fn new(program: &OsStr, environment: &ProcessEnvironment, bounded: BoundedCommand) -> Self {
        Self {
            program: program.to_os_string(),
            socket: None,
            config: None,
            environment: environment.clone(),
            bounded,
        }
    }

    pub fn removing_env_names(self, names: &[String]) -> Self {
        Self {
            environment: self.environment.without(names),
            ..self
        }
    }

    #[cfg(test)]
    pub fn on_test_server(self, socket: &Path) -> Self {
        Self {
            socket: Some(socket.to_path_buf()),
            config: Some(PathBuf::from("/dev/null")),
            ..self
        }
    }

    #[cfg(test)]
    pub fn with_config_file(self, config: &Path) -> Self {
        Self {
            config: Some(config.to_path_buf()),
            ..self
        }
    }

    pub fn on_socket(&self, socket: &Path) -> Self {
        Self {
            socket: Some(socket.to_path_buf()),
            ..self.clone()
        }
    }

    pub fn caller_pane(&self) -> Option<String> {
        self.environment
            .get("TMUX_PANE")
            .map(|pane| pane.to_string_lossy().into_owned())
    }

    pub fn runs_inside_tmux(&self) -> bool {
        self.environment.contains("TMUX")
    }

    pub fn query(&self, sequence: &TmuxSequence) -> Result<TmuxOutput, TmuxError> {
        self.run(
            sequence,
            Call::new(&Self::label(sequence), CallKind::TmuxQuery),
        )
    }

    pub fn change(&self, sequence: &TmuxSequence) -> Result<TmuxOutput, TmuxError> {
        self.run(
            sequence,
            Call::new(&Self::label(sequence), CallKind::TmuxChange),
        )
    }

    pub fn change_with_input(
        &self,
        sequence: &TmuxSequence,
        input: &[u8],
    ) -> Result<TmuxOutput, TmuxError> {
        let call = Call::new(&Self::label(sequence), CallKind::TmuxChange).with_input(input);
        self.run(sequence, call)
    }

    pub fn create(
        &self,
        sequence: &TmuxSequence,
        removed_env_names: &[String],
    ) -> Result<TmuxOutput, TmuxError> {
        let narrowed = self.clone().removing_env_names(removed_env_names);
        narrowed.change(sequence)
    }

    pub fn foreground(&self, sequence: &TmuxSequence) -> Command {
        self.command(sequence)
    }

    pub fn version(&self) -> Result<String, TmuxError> {
        let mut command = Command::new(&self.program);
        command
            .env_clear()
            .envs(self.environment.variables().iter().cloned());
        command.args(["-u", "-V"]);
        let call = Call::new("tmux -V", CallKind::TmuxQuery);
        let output = self.bounded.run(command, &call).map_err(TmuxError::from)?;
        let text = output.stdout_text();
        Ok(text.trim().trim_start_matches("tmux ").to_string())
    }

    pub fn is_inside_client(&self, caller: &CallerProcess) -> bool {
        if !self.runs_inside_tmux() {
            return false;
        }
        let Some(pane) = self.caller_pane() else {
            return true;
        };
        self.pane_shows_caller(&pane, caller).unwrap_or(false)
    }

    fn pane_shows_caller(&self, pane: &str, caller: &CallerProcess) -> Option<bool> {
        let lookup = TmuxSequence::new().command([
            "display-message",
            "-p",
            "-t",
            pane,
            "ringleader-pane #{pane_id} #{pane_pid} #{pane_tty}",
        ]);
        let output = self.query(&lookup).ok()?;
        let mut values = output.first("ringleader-pane")?.splitn(3, ' ');
        let (pane_id, pane_pid, pane_tty) = (values.next()?, values.next()?, values.next()?);
        if pane_id != pane {
            return Some(false);
        }
        let terminal = fs::metadata(pane_tty).ok()?.rdev();
        Some(caller.descends_in_foreground(pane_pid.parse().ok()?, terminal))
    }

    fn run(&self, sequence: &TmuxSequence, call: Call) -> Result<TmuxOutput, TmuxError> {
        let output = self
            .bounded
            .run(self.command(sequence), &call)
            .map_err(TmuxError::from)?;
        Ok(TmuxOutput::new(
            output.succeeded(),
            &output.stdout_text(),
            &output.stderr_text(),
        ))
    }

    fn command(&self, sequence: &TmuxSequence) -> Command {
        let mut command = Command::new(&self.program);
        command
            .env_clear()
            .envs(self.environment.variables().iter().cloned());
        command.arg("-u");
        if !sequence.starts_server() {
            command.arg("-N");
        }
        if let Some(socket) = &self.socket {
            command.arg("-S").arg(socket);
        }
        if let Some(config) = &self.config {
            command.arg("-f").arg(config);
        }
        command.args(sequence.argv());
        command
    }

    fn label(sequence: &TmuxSequence) -> String {
        format!("tmux {}", sequence.first_subcommand())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TmuxError {
    #[error("{subcommand} failed: {detail}")]
    Failed { subcommand: String, detail: String },
    #[error("{subcommand} timed out after {seconds} seconds")]
    TimedOut { subcommand: String, seconds: u64 },
}

impl TmuxError {
    pub fn failed(subcommand: &str, output: &TmuxOutput) -> Self {
        let detail = match output.stderr() {
            "" => "tmux printed no confirmation".to_string(),
            stderr => stderr.to_string(),
        };
        TmuxError::Failed {
            subcommand: subcommand.to_string(),
            detail,
        }
    }

    pub fn is_timeout(&self) -> bool {
        matches!(self, TmuxError::TimedOut { .. })
    }
}

impl From<CommandError> for TmuxError {
    fn from(error: CommandError) -> Self {
        match error {
            CommandError::TimedOut {
                subcommand,
                seconds,
            } => TmuxError::TimedOut {
                subcommand,
                seconds,
            },
            CommandError::Spawn { subcommand, detail } => TmuxError::Failed { subcommand, detail },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::caller_process::ProcessStat;
    use crate::test_support::TmuxTestServer;

    struct PaneFixture {
        server: TmuxTestServer,
        pane: String,
        pane_pid: u32,
        terminal: u64,
    }

    impl PaneFixture {
        const SELF_PID: u32 = 4_000_001;
        const UNUSED_TERMINAL_MAJOR: u64 = 250 << 8;
        const NESTED_TERMINAL: u64 = Self::UNUSED_TERMINAL_MAJOR | 1;
        const OTHER_TERMINAL: u64 = Self::UNUSED_TERMINAL_MAJOR | 2;

        fn live() -> Self {
            let server = TmuxTestServer::with_session("holder");
            let values = server.text(&[
                "display-message",
                "-p",
                "-t",
                "=holder:",
                "#{pane_id} #{pane_pid} #{pane_tty}",
            ]);
            let mut parts = values.split(' ');
            let pane = parts.next().unwrap().to_string();
            let pane_pid = parts.next().unwrap().parse().unwrap();
            let terminal = fs::metadata(parts.next().unwrap()).unwrap().rdev();
            Self {
                server,
                pane,
                pane_pid,
                terminal,
            }
        }

        fn command(&self, pane: Option<&str>) -> TmuxCommand {
            let tmux_value = format!(
                "{},{},0",
                self.server.socket().display(),
                self.server.server_pid()
            );
            let mut extra = vec![("TMUX", tmux_value.as_str())];
            if let Some(pane) = pane {
                extra.push(("TMUX_PANE", pane));
            }
            self.server
                .command_with(&self.server.environment_with(&extra))
        }

        fn inside(&self, chain: Vec<ProcessStat>) -> bool {
            self.command(Some(&self.pane))
                .is_inside_client(&CallerProcess::from_chain(chain))
        }

        fn pane_process(&self, foreground: bool) -> ProcessStat {
            let group = i64::from(self.pane_pid);
            let foreground_group = if foreground { group } else { group + 1 };
            ProcessStat::new(self.pane_pid, group, self.terminal, foreground_group)
        }

        fn process(pid: u32, terminal: u64, foreground: bool) -> ProcessStat {
            let group = i64::from(pid);
            let foreground_group = if foreground { group } else { group + 1 };
            ProcessStat::new(pid, group, terminal, foreground_group)
        }
    }

    #[test]
    fn this_process_in_the_foreground_of_the_pane_is_inside() {
        let fixture = PaneFixture::live();
        let chain = vec![
            PaneFixture::process(PaneFixture::SELF_PID, fixture.terminal, true),
            fixture.pane_process(false),
        ];

        assert!(fixture.inside(chain));
    }

    #[test]
    fn a_foreground_ancestor_whose_child_is_on_a_nested_terminal_is_inside() {
        let fixture = PaneFixture::live();
        let chain = vec![
            PaneFixture::process(PaneFixture::SELF_PID, PaneFixture::NESTED_TERMINAL, true),
            PaneFixture::process(PaneFixture::SELF_PID + 1, 0, false),
            PaneFixture::process(PaneFixture::SELF_PID + 2, fixture.terminal, true),
            fixture.pane_process(false),
        ];

        assert!(fixture.inside(chain));
    }

    #[test]
    fn a_foreground_pane_process_whose_child_has_a_nested_terminal_is_inside() {
        let fixture = PaneFixture::live();
        let chain = vec![
            PaneFixture::process(PaneFixture::SELF_PID, PaneFixture::NESTED_TERMINAL, true),
            fixture.pane_process(true),
        ];

        assert!(fixture.inside(chain));
    }

    #[test]
    fn tmux_without_a_pane_variable_is_inside() {
        let fixture = PaneFixture::live();

        let inside = fixture
            .command(None)
            .is_inside_client(&CallerProcess::from_chain(Vec::new()));

        assert!(inside);
    }

    #[test]
    fn a_background_ancestor_below_the_pane_is_outside() {
        let fixture = PaneFixture::live();
        let chain = vec![
            PaneFixture::process(PaneFixture::SELF_PID, PaneFixture::OTHER_TERMINAL, true),
            PaneFixture::process(PaneFixture::SELF_PID + 1, fixture.terminal, false),
            fixture.pane_process(true),
        ];

        assert!(!fixture.inside(chain));
    }

    #[test]
    fn a_background_ancestor_below_a_foreground_subshell_is_outside() {
        let fixture = PaneFixture::live();
        let chain = vec![
            PaneFixture::process(PaneFixture::SELF_PID, PaneFixture::OTHER_TERMINAL, true),
            PaneFixture::process(PaneFixture::SELF_PID + 1, fixture.terminal, false),
            PaneFixture::process(PaneFixture::SELF_PID + 2, fixture.terminal, true),
            fixture.pane_process(false),
        ];

        assert!(!fixture.inside(chain));
    }

    #[test]
    fn an_unrelated_terminal_is_outside() {
        let fixture = PaneFixture::live();
        let chain = vec![
            PaneFixture::process(PaneFixture::SELF_PID, PaneFixture::OTHER_TERMINAL, true),
            PaneFixture::process(PaneFixture::SELF_PID + 1, PaneFixture::OTHER_TERMINAL, true),
        ];

        assert!(!fixture.inside(chain));
    }

    #[test]
    fn a_stale_pane_variable_is_outside() {
        let fixture = PaneFixture::live();
        let chain = vec![
            PaneFixture::process(PaneFixture::SELF_PID, fixture.terminal, true),
            fixture.pane_process(false),
        ];

        let inside = fixture
            .command(Some("%999"))
            .is_inside_client(&CallerProcess::from_chain(chain));

        assert!(!inside);
    }

    #[test]
    fn no_tmux_variable_is_outside() {
        let server = TmuxTestServer::new();

        assert!(!server.command().is_inside_client(&CallerProcess::read()));
    }

    #[test]
    fn list_keys_against_an_absent_socket_starts_no_server() {
        let server = TmuxTestServer::new();

        let output = server
            .command()
            .query(&TmuxSequence::new().command(["list-keys"]))
            .unwrap();

        assert!(output.reports_no_server());
        assert!(!server.socket().exists());
    }

    #[test]
    fn attach_session_against_an_absent_socket_starts_no_server() {
        let server = TmuxTestServer::new();

        let attach = TmuxSequence::new().command(["attach-session", "-t", "=missing:"]);
        let output = server.command().query(&attach).unwrap();

        assert!(output.reports_no_server());
        assert!(!server.socket().exists());
    }

    #[test]
    fn arguments_ending_in_a_semicolon_arrive_byte_for_byte() {
        let server = TmuxTestServer::with_session("holder");
        let values = ["fix bug;", r"a\;", ";", "x ; y"];

        for (index, value) in values.iter().enumerate() {
            let output_file = server.directory().join(format!("argument-{index}"));
            let script = "printf '%s|%s' \"$1\" \"$2\" > \"$3\"";
            let launch = TmuxSequence::new().command([
                "new-window",
                "-t",
                "=holder:",
                "sh",
                "-c",
                script,
                "sh",
                value,
                "after",
                &output_file.to_string_lossy(),
            ]);
            server.command().change(&launch).unwrap();
            assert!(
                server.wait_for(
                    || fs::read_to_string(&output_file).is_ok_and(|text| !text.is_empty())
                )
            );
            assert_eq!(
                fs::read_to_string(&output_file).unwrap(),
                format!("{value}|after")
            );
        }
    }

    #[test]
    fn start_directories_with_format_characters_are_entered_exactly() {
        let server = TmuxTestServer::with_session("holder");
        let names = ["a#{b}", "c#(d)", "e##f", "g#[h]", "i##[j]", "k#[#{l}", "m#"];

        for name in names {
            let directory = server.directory().join(name);
            fs::create_dir(&directory).unwrap();
            let window = TmuxSequence::new().command([
                OsString::from("new-window"),
                OsString::from("-t"),
                OsString::from("=holder:"),
                OsString::from("-P"),
                OsString::from("-F"),
                OsString::from("ringleader-created #{pane_id}"),
                OsString::from("-c"),
                TmuxSequence::start_directory(&directory),
                OsString::from("sleep"),
                OsString::from("30"),
            ]);
            let output = server.command().change(&window).unwrap();
            let pane = output.first("ringleader-created").unwrap().to_string();
            let expected = directory.to_string_lossy().into_owned();
            let entered = server.wait_for(|| {
                server.text(&["display-message", "-p", "-t", &pane, "#{pane_current_path}"])
                    == expected
            });
            assert!(entered, "the pane for {name} started elsewhere");
        }
    }

    #[test]
    fn a_tmux_version_is_read() {
        let server = TmuxTestServer::new();

        let version = server.command().version().unwrap();

        assert!(version.starts_with('3'));
    }
}
