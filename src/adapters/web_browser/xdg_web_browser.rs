use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use super::null_web_browser::NullWebBrowser;
use crate::adapters::child_process::detached_command::DetachedCommand;
use crate::adapters::host::process_environment::ProcessEnvironment;
use crate::application::ports::{HostProbe, WebBrowser, WebBrowserError};

pub struct XdgWebBrowser {
    program: PathBuf,
    environment: ProcessEnvironment,
}

impl XdgWebBrowser {
    const XDG_OPEN: &'static str = "xdg-open";
    const OUTSIDE_EVERY_WORKTREE: &'static str = "/";

    pub fn detect(host: &dyn HostProbe, environment: &ProcessEnvironment) -> Arc<dyn WebBrowser> {
        if !environment.has_graphical_display() {
            return Arc::new(Self::without_display());
        }
        match host.resolve_command(Self::XDG_OPEN) {
            Some(program) => Arc::new(Self {
                program,
                environment: environment.outside_tmux(),
            }),
            None => Arc::new(Self::without_xdg_open()),
        }
    }

    fn without_display() -> NullWebBrowser {
        let reason = ProcessEnvironment::no_display_reason();
        NullWebBrowser::new(WebBrowserError::NoDisplay, &reason)
    }

    fn without_xdg_open() -> NullWebBrowser {
        let reason = format!("{} is not on PATH", Self::XDG_OPEN);
        NullWebBrowser::new(WebBrowserError::NoBrowser, &reason)
    }

    fn command(&self, url: &str) -> DetachedCommand {
        let mut command = Command::new(&self.program);
        command
            .arg(url)
            .current_dir(Self::OUTSIDE_EVERY_WORKTREE)
            .env_clear()
            .envs(self.environment.variables().iter().cloned());
        DetachedCommand::new(command)
    }
}

impl WebBrowser for XdgWebBrowser {
    fn open(&self, url: &str) -> Result<(), WebBrowserError> {
        self.command(url).start().map_err(|error| {
            log::warn!("cannot start {}: {error}", self.program.display());
            WebBrowserError::StartFailed
        })
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::Path;

    use super::*;
    use crate::adapters::host::system_host_probe::SystemHostProbe;
    use crate::test_support::{ExecutableScript, LogCapture, RecordingPrograms};

    struct BrowserFixture {
        programs: RecordingPrograms,
    }

    impl BrowserFixture {
        const XDG_OPEN: &'static str = "xdg-open";
        const URL: &'static str = "https://github.example.com/example-org/example-project/pull/142";
        const WAYLAND: (&'static str, &'static str) = ("WAYLAND_DISPLAY", "wayland-1");
        const MISSING_INTERPRETER: &'static str = "#!/nonexistent/interpreter\n";

        fn new() -> Self {
            Self {
                programs: RecordingPrograms::new(),
            }
        }

        fn with_xdg_open() -> Self {
            let fixture = Self::new();
            fixture.programs.install(Self::XDG_OPEN);
            fixture
        }

        fn program_directory(&self) -> PathBuf {
            self.programs.program_directory()
        }

        fn pid(&self, name: &str) -> Option<u32> {
            self.programs.pid(name)
        }

        fn never_ran(&self, name: &str) -> bool {
            self.programs.never_ran(name)
        }

        fn ran_in(&self, name: &str) -> Option<PathBuf> {
            self.programs.ran_in(name)
        }

        fn environment_of(&self, name: &str) -> Vec<String> {
            self.programs.environment_of(name)
        }

        fn streams_of(&self, name: &str) -> String {
            self.programs.streams_of(name)
        }

        fn arguments_of(&self, name: &str) -> String {
            self.programs.arguments_of(name)
        }

        fn session_of(&self, name: &str) -> Option<u32> {
            self.programs.session_of(name)
        }

        fn install_xdg_open_without_its_interpreter(&self) -> PathBuf {
            let program = self.programs.program_directory().join(Self::XDG_OPEN);
            ExecutableScript::write(&program, Self::MISSING_INTERPRETER);
            program
        }

        fn open_with(&self, variables: &[(&str, &str)]) -> Result<(), WebBrowserError> {
            let environment = self.programs.environment(variables);
            let host = SystemHostProbe::new(&environment, OsStr::new("tmux"));
            XdgWebBrowser::detect(&host, &environment).open(Self::URL)
        }
    }

    #[test]
    fn xdg_open_on_the_path_opens_the_url() {
        let fixture = BrowserFixture::with_xdg_open();

        fixture.open_with(&[BrowserFixture::WAYLAND]).unwrap();

        assert_eq!(
            fixture.arguments_of(BrowserFixture::XDG_OPEN),
            format!("{}\n", BrowserFixture::URL)
        );
    }

    #[test]
    fn the_browser_reads_and_writes_none_of_the_terminal() {
        let fixture = BrowserFixture::with_xdg_open();

        fixture.open_with(&[BrowserFixture::WAYLAND]).unwrap();

        assert_eq!(
            fixture.streams_of(BrowserFixture::XDG_OPEN),
            "null\nnull\nnull\n"
        );
    }

    #[test]
    fn the_browser_leads_a_session_of_its_own() {
        let fixture = BrowserFixture::with_xdg_open();

        fixture.open_with(&[BrowserFixture::WAYLAND]).unwrap();

        let pid = fixture.pid(BrowserFixture::XDG_OPEN);
        assert!(pid.is_some());
        assert_eq!(fixture.session_of(BrowserFixture::XDG_OPEN), pid);
    }

    #[test]
    fn the_browser_gets_the_environment_without_the_tmux_variables() {
        let fixture = BrowserFixture::with_xdg_open();

        fixture
            .open_with(&[
                BrowserFixture::WAYLAND,
                ("TMUX", "/tmp/tmux-1000/default,1,0"),
                ("TMUX_PANE", "%3"),
                ("EXAMPLE_SETTING", "kept"),
            ])
            .unwrap();

        assert_eq!(
            fixture.environment_of(BrowserFixture::XDG_OPEN),
            [
                "EXAMPLE_SETTING=kept".to_string(),
                format!("PATH={}", fixture.program_directory().display()),
                "WAYLAND_DISPLAY=wayland-1".to_string(),
            ]
        );
    }

    #[test]
    fn the_browser_starts_outside_every_worktree_so_no_delete_counts_it_as_a_worktree_process() {
        let fixture = BrowserFixture::with_xdg_open();

        fixture.open_with(&[BrowserFixture::WAYLAND]).unwrap();

        assert_eq!(
            fixture.ran_in(BrowserFixture::XDG_OPEN),
            Some(Path::new("/").to_path_buf())
        );
    }

    #[test]
    fn an_x11_display_is_enough() {
        let fixture = BrowserFixture::with_xdg_open();

        fixture.open_with(&[("DISPLAY", ":0")]).unwrap();

        assert!(fixture.pid(BrowserFixture::XDG_OPEN).is_some());
    }

    #[test]
    fn without_a_graphical_display_nothing_starts_and_only_the_log_names_the_variables() {
        let log = LogCapture::installed();
        let fixture = BrowserFixture::with_xdg_open();

        let error = fixture
            .open_with(&[("WAYLAND_DISPLAY", ""), ("DISPLAY", "")])
            .unwrap_err();

        assert_eq!(error, WebBrowserError::NoDisplay);
        assert!(fixture.never_ran(BrowserFixture::XDG_OPEN));
        assert_eq!(
            log.count_containing(
                "cannot open a web browser: WAYLAND_DISPLAY and DISPLAY are unset or empty"
            ),
            1
        );
    }

    #[test]
    fn without_xdg_open_nothing_opens_and_only_the_log_names_xdg_open() {
        let log = LogCapture::installed();
        let fixture = BrowserFixture::new();

        let error = fixture.open_with(&[BrowserFixture::WAYLAND]).unwrap_err();

        assert_eq!(error, WebBrowserError::NoBrowser);
        assert_eq!(
            log.count_containing("cannot open a web browser: xdg-open is not on PATH"),
            1
        );
    }

    #[test]
    fn a_browser_that_cannot_start_fails_in_the_ports_terms() {
        let fixture = BrowserFixture::new();
        fixture.install_xdg_open_without_its_interpreter();

        let error = fixture.open_with(&[BrowserFixture::WAYLAND]).unwrap_err();

        assert_eq!(error, WebBrowserError::StartFailed);
    }

    #[test]
    fn a_browser_that_cannot_start_logs_the_program_and_the_reason() {
        let log = LogCapture::installed();
        let fixture = BrowserFixture::new();
        let program = fixture.install_xdg_open_without_its_interpreter();
        let warning = format!(
            "cannot start {}: No such file or directory",
            program.display()
        );

        fixture.open_with(&[BrowserFixture::WAYLAND]).unwrap_err();

        assert_eq!(log.count_containing(&warning), 1);
    }
}
