use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::thread;

use super::bounded_command::BoundedCommand;
use super::null_terminal_emulator::NullTerminalEmulator;
use super::process_environment::ProcessEnvironment;
use crate::application::ports::{HostProbe, TerminalEmulator, TerminalEmulatorError};

pub struct SystemTerminalEmulator {
    program: TerminalProgram,
    environment: ProcessEnvironment,
}

impl SystemTerminalEmulator {
    const XDG_TERMINAL_EXEC: &'static str = "xdg-terminal-exec";

    pub fn detect(
        host: &dyn HostProbe,
        environment: &ProcessEnvironment,
    ) -> Arc<dyn TerminalEmulator> {
        match Self::program(host, environment) {
            Ok(program) => Arc::new(Self::new(program, environment)),
            Err(missing) => Arc::new(NullTerminalEmulator::new(missing)),
        }
    }

    fn new(program: TerminalProgram, environment: &ProcessEnvironment) -> Self {
        Self {
            program,
            environment: environment.outside_tmux(),
        }
    }

    fn program(
        host: &dyn HostProbe,
        environment: &ProcessEnvironment,
    ) -> Result<TerminalProgram, TerminalEmulatorError> {
        if !environment.has_graphical_display() {
            return Err(Self::no_display());
        }
        host.resolve_command(Self::XDG_TERMINAL_EXEC)
            .map(TerminalProgram::xdg_terminal_exec)
            .or_else(|| environment.terminal_emulator().map(TerminalProgram::named))
            .ok_or_else(Self::no_terminal)
    }

    fn no_display() -> TerminalEmulatorError {
        let displays = ProcessEnvironment::display_variables().join(" and ");
        TerminalEmulatorError::NoDisplay {
            reason: format!("{displays} are unset or empty"),
        }
    }

    fn no_terminal() -> TerminalEmulatorError {
        let terminal = ProcessEnvironment::terminal_emulator_variable();
        TerminalEmulatorError::NoTerminal {
            reason: format!(
                "{} is not on PATH and ${terminal} is unset or empty",
                Self::XDG_TERMINAL_EXEC
            ),
        }
    }

    fn command(&self, directory: &Path) -> Command {
        let mut command = self.program.command_in(directory);
        command
            .env_clear()
            .envs(self.environment.variables().iter().cloned())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        BoundedCommand::start_in_own_session(&mut command);
        command
    }

    fn reap_in_background(&self, mut child: Child) {
        let program = self.shown_program();
        thread::spawn(move || Self::warn_on_failure(&program, child.wait()));
    }

    fn warn_on_failure(program: &str, exit: io::Result<ExitStatus>) {
        match exit {
            Ok(status) if !status.success() => log::warn!("{program} ended with {status}"),
            Ok(_) => {}
            Err(error) => log::warn!("cannot wait for {program}: {error}"),
        }
    }

    fn start_failure(&self, error: &io::Error) -> TerminalEmulatorError {
        TerminalEmulatorError::StartFailed {
            program: self.shown_program(),
            reason: error.to_string(),
        }
    }

    fn shown_program(&self) -> String {
        self.program.shown()
    }
}

impl TerminalEmulator for SystemTerminalEmulator {
    fn open_window(&self, directory: &Path) -> Result<(), TerminalEmulatorError> {
        let child = self
            .command(directory)
            .spawn()
            .map_err(|error| self.start_failure(&error))?;
        self.reap_in_background(child);
        Ok(())
    }
}

struct TerminalProgram {
    path: OsString,
    directory_option: Option<&'static str>,
}

impl TerminalProgram {
    const XDG_TERMINAL_EXEC_DIRECTORY: &'static str = "--dir=";

    fn xdg_terminal_exec(path: PathBuf) -> Self {
        Self {
            path: path.into_os_string(),
            directory_option: Some(Self::XDG_TERMINAL_EXEC_DIRECTORY),
        }
    }

    fn named(name: &OsStr) -> Self {
        Self {
            path: name.to_os_string(),
            directory_option: None,
        }
    }

    fn command_in(&self, directory: &Path) -> Command {
        let mut command = Command::new(&self.path);
        command
            .current_dir(directory)
            .args(self.directory_argument(directory));
        command
    }

    fn directory_argument(&self, directory: &Path) -> Option<OsString> {
        self.directory_option.map(|option| {
            let mut argument = OsString::from(option);
            argument.push(directory);
            argument
        })
    }

    fn shown(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::adapters::live_process::LiveProcess;
    use crate::adapters::system_host_probe::SystemHostProbe;
    use crate::test_support::{Deadline, LogCapture};

    struct TerminalFixture {
        directory: tempfile::TempDir,
    }

    impl TerminalFixture {
        const XDG_TERMINAL_EXEC: &'static str = "xdg-terminal-exec";
        const NAMED_TERMINAL: &'static str = "example terminal";
        const WAYLAND: (&'static str, &'static str) = ("WAYLAND_DISPLAY", "wayland-1");
        const EXECUTABLE: u32 = 0o755;
        const NOT_EXECUTABLE: u32 = 0o644;
        const SUCCESS: &'static str = "exit 0";
        const FAILURE: &'static str = "exit 3";
        const LINGERING_SECONDS: u32 = 30;
        const RECORDING: &'static str = r#"#!/bin/sh
record='{record}'
'{cat}' /proc/$$/environ > "$record/environ"
read -r stat < /proc/$$/stat
echo "$stat" > "$record/stat"
pwd -P > "$record/cwd"
: > "$record/arguments"
for argument in "$@"; do
  printf '%s\n' "$argument" >> "$record/arguments"
done
for descriptor in 0 1 2; do
  if [ /proc/$$/fd/$descriptor -ef /dev/null ]; then
    echo null >> "$record/streams"
  else
    echo terminal >> "$record/streams"
  fi
done
echo $$ > "$record/pid"
{ending}
"#;

        fn new() -> Self {
            let fixture = Self {
                directory: tempfile::tempdir().unwrap(),
            };
            fs::create_dir(fixture.programs()).unwrap();
            fs::create_dir(fixture.worktree()).unwrap();
            fixture
        }

        fn programs(&self) -> PathBuf {
            self.directory.path().join("bin")
        }

        fn worktree(&self) -> PathBuf {
            self.directory.path().join("worktree")
        }

        fn install(&self, name: &str) -> PathBuf {
            self.install_script(name, Self::SUCCESS, Self::EXECUTABLE)
        }

        fn install_failing(&self, name: &str) -> PathBuf {
            self.install_script(name, Self::FAILURE, Self::EXECUTABLE)
        }

        fn install_unexecutable(&self, name: &str) -> PathBuf {
            self.install_script(name, Self::SUCCESS, Self::NOT_EXECUTABLE)
        }

        fn install_lingering(&self, name: &str) -> PathBuf {
            let sleep = Self::tool("sleep").display().to_string();
            let ending = format!("exec '{sleep}' {}", Self::LINGERING_SECONDS);
            self.install_script(name, &ending, Self::EXECUTABLE)
        }

        fn install_script(&self, name: &str, ending: &str, mode: u32) -> PathBuf {
            let record = self.record_directory(name);
            fs::create_dir_all(&record).unwrap();
            let script = Self::RECORDING
                .replace("{record}", &record.to_string_lossy())
                .replace("{cat}", &Self::tool("cat").to_string_lossy())
                .replace("{ending}", ending);
            let program = self.programs().join(name);
            fs::write(&program, script).unwrap();
            fs::set_permissions(&program, fs::Permissions::from_mode(mode)).unwrap();
            program
        }

        fn tool(name: &str) -> PathBuf {
            let search_path = std::env::var_os("PATH").unwrap_or_default();
            std::env::split_paths(&search_path)
                .map(|directory| directory.join(name))
                .find(|candidate| candidate.is_file())
                .expect("the test PATH holds the tool")
        }

        fn record_directory(&self, name: &str) -> PathBuf {
            self.directory.path().join("record").join(name)
        }

        fn environment(&self, variables: &[(&str, &str)]) -> ProcessEnvironment {
            let search_path = ("PATH".into(), self.programs().into_os_string());
            let variables = variables
                .iter()
                .map(|(name, value)| (OsString::from(name), OsString::from(value)));
            ProcessEnvironment::new(
                std::iter::once(search_path).chain(variables).collect(),
                None,
            )
        }

        fn open_with(&self, variables: &[(&str, &str)]) -> Result<(), TerminalEmulatorError> {
            let environment = self.environment(variables);
            let host = SystemHostProbe::new(&environment, OsStr::new("tmux"));
            SystemTerminalEmulator::detect(&host, &environment).open_window(&self.worktree())
        }

        fn pid(&self, name: &str) -> Option<u32> {
            let mut pid = None;
            Deadline::for_background_work().wait_until(|| {
                pid = self.recorded(name, "pid").trim().parse().ok();
                pid.is_some()
            });
            pid
        }

        fn recorded(&self, name: &str, file: &str) -> String {
            fs::read_to_string(self.record_directory(name).join(file)).unwrap_or_default()
        }

        fn ran_in(&self, name: &str) -> Option<PathBuf> {
            self.pid(name)?;
            Some(PathBuf::from(self.recorded(name, "cwd").trim()))
        }

        fn environment_of(&self, name: &str) -> Vec<String> {
            let mut variables: Vec<String> = self
                .recorded_once_started(name, "environ")
                .split('\0')
                .filter(|variable| !variable.is_empty())
                .map(str::to_string)
                .collect();
            variables.sort();
            variables
        }

        fn streams_of(&self, name: &str) -> String {
            self.recorded_once_started(name, "streams")
        }

        fn arguments_of(&self, name: &str) -> String {
            self.recorded_once_started(name, "arguments")
        }

        fn recorded_once_started(&self, name: &str, file: &str) -> String {
            self.pid(name);
            self.recorded(name, file)
        }

        fn session_of(&self, name: &str) -> Option<u32> {
            self.pid(name)?;
            let stat = self.recorded(name, "stat");
            let (_, fields) = stat.rsplit_once(')')?;
            fields.split_whitespace().nth(3)?.parse().ok()
        }

        fn canonical_worktree(&self) -> PathBuf {
            fs::canonicalize(self.worktree()).unwrap()
        }
    }

    #[test]
    fn xdg_terminal_exec_on_the_path_opens_in_the_directory() {
        let fixture = TerminalFixture::new();
        fixture.install(TerminalFixture::XDG_TERMINAL_EXEC);

        fixture.open_with(&[TerminalFixture::WAYLAND]).unwrap();

        assert_eq!(
            fixture.ran_in(TerminalFixture::XDG_TERMINAL_EXEC),
            Some(fixture.canonical_worktree())
        );
    }

    #[test]
    fn xdg_terminal_exec_passes_the_directory_on_to_a_running_terminal() {
        let fixture = TerminalFixture::new();
        fixture.install(TerminalFixture::XDG_TERMINAL_EXEC);

        fixture.open_with(&[TerminalFixture::WAYLAND]).unwrap();

        assert_eq!(
            fixture.arguments_of(TerminalFixture::XDG_TERMINAL_EXEC),
            format!("--dir={}\n", fixture.worktree().display())
        );
    }

    #[test]
    fn the_window_opens_without_waiting_for_the_program_to_end() {
        let fixture = TerminalFixture::new();
        fixture.install_lingering(TerminalFixture::XDG_TERMINAL_EXEC);

        fixture.open_with(&[TerminalFixture::WAYLAND]).unwrap();

        let pid = fixture.pid(TerminalFixture::XDG_TERMINAL_EXEC).unwrap();
        let process = LiveProcess::read(pid).unwrap();
        let running = process.is_alive();
        process.signal(libc::SIGTERM).unwrap();
        assert!(running);
    }

    #[test]
    fn xdg_terminal_exec_wins_over_the_terminal_variable() {
        let fixture = TerminalFixture::new();
        fixture.install(TerminalFixture::XDG_TERMINAL_EXEC);
        let named = fixture.install(TerminalFixture::NAMED_TERMINAL);

        fixture
            .open_with(&[
                TerminalFixture::WAYLAND,
                ("TERMINAL", &named.to_string_lossy()),
            ])
            .unwrap();

        assert!(fixture.pid(TerminalFixture::XDG_TERMINAL_EXEC).is_some());
        assert_eq!(fixture.recorded(TerminalFixture::NAMED_TERMINAL, "pid"), "");
    }

    #[test]
    fn without_xdg_terminal_exec_the_terminal_variable_names_one_program() {
        let fixture = TerminalFixture::new();
        let named = fixture.install(TerminalFixture::NAMED_TERMINAL);

        fixture
            .open_with(&[
                TerminalFixture::WAYLAND,
                ("TERMINAL", &named.to_string_lossy()),
            ])
            .unwrap();

        assert_eq!(
            fixture.ran_in(TerminalFixture::NAMED_TERMINAL),
            Some(fixture.canonical_worktree())
        );
        assert_eq!(fixture.arguments_of(TerminalFixture::NAMED_TERMINAL), "");
    }

    #[test]
    fn a_bare_terminal_name_is_found_on_the_path_ringleader_started_with() {
        let fixture = TerminalFixture::new();
        fixture.install("example-terminal");

        fixture
            .open_with(&[TerminalFixture::WAYLAND, ("TERMINAL", "example-terminal")])
            .unwrap();

        assert!(fixture.pid("example-terminal").is_some());
    }

    #[test]
    fn without_xdg_terminal_exec_or_a_terminal_variable_nothing_opens() {
        let fixture = TerminalFixture::new();

        let error = fixture
            .open_with(&[TerminalFixture::WAYLAND, ("TERMINAL", "")])
            .unwrap_err();

        assert_eq!(error, SystemTerminalEmulator::no_terminal());
        assert_eq!(
            error.to_string(),
            "no terminal to open, xdg-terminal-exec is not on PATH and $TERMINAL is unset or empty"
        );
    }

    #[test]
    fn without_a_graphical_display_nothing_starts() {
        let fixture = TerminalFixture::new();
        fixture.install(TerminalFixture::XDG_TERMINAL_EXEC);

        let error = fixture
            .open_with(&[("WAYLAND_DISPLAY", ""), ("DISPLAY", "")])
            .unwrap_err();

        assert_eq!(error, SystemTerminalEmulator::no_display());
        assert_eq!(
            error.to_string(),
            "no graphical display is available, WAYLAND_DISPLAY and DISPLAY are unset or empty"
        );
        assert_eq!(
            fs::read_dir(fixture.record_directory(TerminalFixture::XDG_TERMINAL_EXEC))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn an_x11_display_is_enough() {
        let fixture = TerminalFixture::new();
        fixture.install(TerminalFixture::XDG_TERMINAL_EXEC);

        fixture.open_with(&[("DISPLAY", ":0")]).unwrap();

        assert!(fixture.pid(TerminalFixture::XDG_TERMINAL_EXEC).is_some());
    }

    #[test]
    fn the_program_gets_the_environment_without_the_tmux_variables() {
        let fixture = TerminalFixture::new();
        fixture.install(TerminalFixture::XDG_TERMINAL_EXEC);

        fixture
            .open_with(&[
                TerminalFixture::WAYLAND,
                ("TMUX", "/tmp/tmux-1000/default,1,0"),
                ("TMUX_PANE", "%3"),
                ("EXAMPLE_SETTING", "kept"),
            ])
            .unwrap();

        assert_eq!(
            fixture.environment_of(TerminalFixture::XDG_TERMINAL_EXEC),
            [
                "EXAMPLE_SETTING=kept".to_string(),
                format!("PATH={}", fixture.programs().display()),
                "WAYLAND_DISPLAY=wayland-1".to_string(),
            ]
        );
    }

    #[test]
    fn the_program_reads_and_writes_none_of_the_terminal() {
        let fixture = TerminalFixture::new();
        fixture.install(TerminalFixture::XDG_TERMINAL_EXEC);

        fixture.open_with(&[TerminalFixture::WAYLAND]).unwrap();

        assert_eq!(
            fixture.streams_of(TerminalFixture::XDG_TERMINAL_EXEC),
            "null\nnull\nnull\n"
        );
    }

    #[test]
    fn the_program_leads_a_session_of_its_own() {
        let fixture = TerminalFixture::new();
        fixture.install(TerminalFixture::XDG_TERMINAL_EXEC);

        fixture.open_with(&[TerminalFixture::WAYLAND]).unwrap();

        let pid = fixture.pid(TerminalFixture::XDG_TERMINAL_EXEC);
        assert!(pid.is_some());
        assert_eq!(fixture.session_of(TerminalFixture::XDG_TERMINAL_EXEC), pid);
    }

    #[test]
    fn an_ended_program_is_reaped() {
        let fixture = TerminalFixture::new();
        fixture.install(TerminalFixture::XDG_TERMINAL_EXEC);

        fixture.open_with(&[TerminalFixture::WAYLAND]).unwrap();

        let pid = fixture.pid(TerminalFixture::XDG_TERMINAL_EXEC).unwrap();
        let process = PathBuf::from(format!("/proc/{pid}"));
        assert!(Deadline::for_background_work().wait_until(|| !process.exists()));
    }

    #[test]
    fn a_failed_exit_is_logged_with_the_program_and_its_status() {
        let capture = LogCapture::installed();
        let fixture = TerminalFixture::new();
        let program = fixture.install_failing(TerminalFixture::XDG_TERMINAL_EXEC);
        let warning = format!("{} ended with exit status: 3", program.display());

        fixture.open_with(&[TerminalFixture::WAYLAND]).unwrap();

        assert!(
            Deadline::for_background_work().wait_until(|| capture.count_containing(&warning) == 1)
        );
    }

    #[test]
    fn a_program_that_cannot_start_is_named_with_the_reason() {
        let fixture = TerminalFixture::new();
        let named = fixture.install_unexecutable(TerminalFixture::NAMED_TERMINAL);

        let error = fixture
            .open_with(&[
                TerminalFixture::WAYLAND,
                ("TERMINAL", &named.to_string_lossy()),
            ])
            .unwrap_err();

        assert!(error.to_string().starts_with(&format!(
            "cannot start {}: Permission denied",
            named.display()
        )));
    }
}
