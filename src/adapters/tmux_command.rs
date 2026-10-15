use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::Command;

use super::bounded_command::{BoundedCommand, Call};
use super::deadlines::{CallKind, Deadlines};
use super::process_environment::ProcessEnvironment;
use super::server_files::ServerFiles;
use super::tmux_error::TmuxError;
use super::tmux_output::TmuxOutput;
use super::tmux_sequence::TmuxSequence;
use crate::application::DiagnosticPaths;

#[derive(Debug, Clone)]
pub struct TmuxCommand {
    program: OsString,
    server: ServerFiles,
    environment: ProcessEnvironment,
    caller: CallerIdentity,
    bounded: BoundedCommand,
}

impl TmuxCommand {
    pub fn new(program: &OsStr, server: &ServerFiles, environment: &ProcessEnvironment) -> Self {
        Self {
            program: program.to_os_string(),
            server: server.clone(),
            environment: environment.outside_tmux(),
            caller: CallerIdentity::of(environment),
            bounded: BoundedCommand::new(Deadlines::standard()),
        }
    }

    pub fn removing_env_names(self, names: &[String]) -> Self {
        Self {
            environment: self.environment.without(names),
            ..self
        }
    }

    #[cfg(test)]
    pub fn with_deadlines(self, deadlines: Deadlines) -> Self {
        Self {
            bounded: BoundedCommand::new(deadlines),
            ..self
        }
    }

    pub fn server(&self) -> &ServerFiles {
        &self.server
    }

    pub fn caller_pane(&self) -> Option<String> {
        let socket = self.caller.socket()?;
        self.caller
            .pane()
            .filter(|_| self.server.is_reached_through(&socket))
    }

    pub fn caller_in_other_tmux(&self) -> bool {
        self.caller
            .socket()
            .is_some_and(|socket| !self.server.is_reached_through(&socket))
    }

    pub fn caller_terminal(&self) -> Option<&OsStr> {
        self.environment.terminal()
    }

    pub fn caller_shell(&self) -> String {
        self.caller.shell()
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
        command.arg("-S").arg(self.server.socket());
        command.arg("-f").arg(self.server.config());
        command.args(sequence.argv());
        command
    }

    fn label(sequence: &TmuxSequence) -> String {
        sequence.first_subcommand()
    }
}

#[derive(Debug, Clone)]
struct CallerIdentity {
    tmux: Option<OsString>,
    pane: Option<String>,
    shell: String,
}

impl CallerIdentity {
    const TMUX: &'static str = "TMUX";
    const TMUX_PANE: &'static str = "TMUX_PANE";

    fn of(environment: &ProcessEnvironment) -> Self {
        Self {
            tmux: environment.get(Self::TMUX).map(OsStr::to_os_string),
            pane: environment
                .get(Self::TMUX_PANE)
                .map(|pane| pane.to_string_lossy().into_owned()),
            shell: environment.shell(),
        }
    }

    fn socket(&self) -> Option<PathBuf> {
        let value = self.tmux.as_ref()?.to_string_lossy().into_owned();
        let socket = value.split(DiagnosticPaths::TMUX_FIELD_SEPARATOR).next()?;
        (!socket.is_empty()).then(|| PathBuf::from(socket))
    }

    fn pane(&self) -> Option<String> {
        self.pane.clone()
    }

    fn shell(&self) -> String {
        self.shell.clone()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;
    use crate::test_support::TmuxTestServer;

    struct CallerFixture;

    impl CallerFixture {
        fn tmux_value(socket: &Path) -> String {
            format!("{},4000001,0", socket.display())
        }

        fn command(server: &TmuxTestServer, socket: &Path) -> TmuxCommand {
            let tmux_value = Self::tmux_value(socket);
            let environment =
                server.environment_with(&[("TMUX", &tmux_value), ("TMUX_PANE", "%0")]);
            server.command_with(&environment)
        }

        fn arguments(command: &Command) -> Vec<String> {
            command
                .get_args()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect()
        }
    }

    #[test]
    fn every_call_names_the_private_socket_and_config() {
        let server = TmuxTestServer::new();

        let attach = server.command().foreground(&TmuxSequence::new().command([
            "attach-session",
            "-t",
            "%3",
        ]));

        let arguments = CallerFixture::arguments(&attach);
        let socket = server.socket().display().to_string();
        let config = server.config().display().to_string();
        assert_eq!(
            arguments,
            [
                "-u",
                "-N",
                "-S",
                &socket,
                "-f",
                &config,
                "attach-session",
                "-t",
                "%3"
            ]
        );
    }

    #[test]
    fn the_attach_child_never_inherits_the_callers_tmux_variables() {
        let server = TmuxTestServer::new();
        let command = CallerFixture::command(&server, server.socket());

        let attach = command.foreground(&TmuxSequence::new().command(["attach-session"]));

        let names: Vec<&OsStr> = attach.get_envs().map(|(name, _)| name).collect();
        assert!(!names.contains(&OsStr::new("TMUX")));
        assert!(!names.contains(&OsStr::new("TMUX_PANE")));
        assert!(names.contains(&OsStr::new("PATH")));
    }

    #[test]
    fn the_caller_pane_comes_from_a_tmux_variable_naming_the_private_socket() {
        let server = TmuxTestServer::new();

        let pane = CallerFixture::command(&server, server.socket()).caller_pane();

        assert_eq!(pane.as_deref(), Some("%0"));
    }

    #[test]
    fn a_pane_of_another_tmux_server_is_never_the_caller_pane() {
        let server = TmuxTestServer::new();
        let users_socket = server.directory().join("users-tmux.sock");

        let pane = CallerFixture::command(&server, &users_socket).caller_pane();

        assert_eq!(pane, None);
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
