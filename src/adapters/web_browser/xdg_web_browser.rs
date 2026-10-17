use std::io;
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
        match Self::program(host, environment) {
            Ok(program) => Arc::new(Self {
                program,
                environment: environment.outside_tmux(),
            }),
            Err(missing) => Arc::new(NullWebBrowser::new(missing)),
        }
    }

    fn program(
        host: &dyn HostProbe,
        environment: &ProcessEnvironment,
    ) -> Result<PathBuf, WebBrowserError> {
        if !environment.has_graphical_display() {
            return Err(Self::no_display());
        }
        host.resolve_command(Self::XDG_OPEN)
            .ok_or_else(Self::no_browser)
    }

    fn no_display() -> WebBrowserError {
        WebBrowserError::NoDisplay {
            reason: ProcessEnvironment::no_display_reason(),
        }
    }

    fn no_browser() -> WebBrowserError {
        WebBrowserError::NoBrowser {
            reason: format!("{} is not on PATH", Self::XDG_OPEN),
        }
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

    fn start_failure(&self, error: &io::Error) -> WebBrowserError {
        WebBrowserError::StartFailed {
            reason: format!("{}: {error}", self.program.display()),
        }
    }
}

impl WebBrowser for XdgWebBrowser {
    fn open(&self, url: &str) -> Result<(), WebBrowserError> {
        self.command(url)
            .start()
            .map_err(|error| self.start_failure(&error))
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::Path;

    use super::*;
    use crate::adapters::host::system_host_probe::SystemHostProbe;
    use crate::test_support::{ExecutableScript, RecordingPrograms};

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

        fn programs(&self) -> &RecordingPrograms {
            &self.programs
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
            fixture.programs().arguments_of(BrowserFixture::XDG_OPEN),
            format!("{}\n", BrowserFixture::URL)
        );
    }

    #[test]
    fn the_browser_reads_and_writes_none_of_the_terminal() {
        let fixture = BrowserFixture::with_xdg_open();

        fixture.open_with(&[BrowserFixture::WAYLAND]).unwrap();

        assert_eq!(
            fixture.programs().streams_of(BrowserFixture::XDG_OPEN),
            "null\nnull\nnull\n"
        );
    }

    #[test]
    fn the_browser_leads_a_session_of_its_own() {
        let fixture = BrowserFixture::with_xdg_open();

        fixture.open_with(&[BrowserFixture::WAYLAND]).unwrap();

        let pid = fixture.programs().pid(BrowserFixture::XDG_OPEN);
        assert!(pid.is_some());
        assert_eq!(fixture.programs().session_of(BrowserFixture::XDG_OPEN), pid);
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
            fixture.programs().environment_of(BrowserFixture::XDG_OPEN),
            [
                "EXAMPLE_SETTING=kept".to_string(),
                format!("PATH={}", fixture.programs().program_directory().display()),
                "WAYLAND_DISPLAY=wayland-1".to_string(),
            ]
        );
    }

    #[test]
    fn the_browser_starts_outside_every_worktree_so_no_delete_counts_it_as_a_worktree_process() {
        let fixture = BrowserFixture::with_xdg_open();

        fixture.open_with(&[BrowserFixture::WAYLAND]).unwrap();

        assert_eq!(
            fixture.programs().ran_in(BrowserFixture::XDG_OPEN),
            Some(Path::new("/").to_path_buf())
        );
    }

    #[test]
    fn an_x11_display_is_enough() {
        let fixture = BrowserFixture::with_xdg_open();

        fixture.open_with(&[("DISPLAY", ":0")]).unwrap();

        assert!(fixture.programs().pid(BrowserFixture::XDG_OPEN).is_some());
    }

    #[test]
    fn without_a_graphical_display_nothing_starts() {
        let fixture = BrowserFixture::with_xdg_open();

        let error = fixture
            .open_with(&[("WAYLAND_DISPLAY", ""), ("DISPLAY", "")])
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "no graphical display is available, WAYLAND_DISPLAY and DISPLAY are unset or empty"
        );
        assert!(fixture.programs().never_ran(BrowserFixture::XDG_OPEN));
    }

    #[test]
    fn without_xdg_open_nothing_opens() {
        let fixture = BrowserFixture::new();

        let error = fixture.open_with(&[BrowserFixture::WAYLAND]).unwrap_err();

        assert_eq!(
            error.to_string(),
            "no web browser to open, xdg-open is not on PATH"
        );
    }

    #[test]
    fn a_browser_that_cannot_start_is_named_with_the_reason() {
        let fixture = BrowserFixture::new();
        let program = fixture.install_xdg_open_without_its_interpreter();

        let error = fixture.open_with(&[BrowserFixture::WAYLAND]).unwrap_err();

        assert!(error.to_string().starts_with(&format!(
            "cannot start the web browser, {}: No such file or directory",
            program.display()
        )));
    }
}
