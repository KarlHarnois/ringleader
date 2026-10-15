use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::path_option::PathOption;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalProgram {
    XdgTerminalExec(PathBuf),
    Ghostty(OsString),
    Named(OsString),
}

impl TerminalProgram {
    const GHOSTTY: &'static str = "ghostty";
    const GHOSTTY_NEW_WINDOW: &'static str = "+new-window";
    const GHOSTTY_DIRECTORY: PathOption = PathOption::new("--working-directory=");
    const XDG_TERMINAL_EXEC_DIRECTORY: PathOption = PathOption::new("--dir=");

    pub fn named(name: &OsStr) -> Self {
        if Path::new(name).file_name() == Some(OsStr::new(Self::GHOSTTY)) {
            return Self::Ghostty(name.to_os_string());
        }
        Self::Named(name.to_os_string())
    }

    pub fn to_start(&self, terminal_of: impl FnOnce(&Path) -> Option<OsString>) -> Self {
        let Self::XdgTerminalExec(launcher) = self else {
            return self.clone();
        };
        match terminal_of(launcher).map(|terminal| Self::named(&terminal)) {
            Some(ghostty @ Self::Ghostty(_)) => ghostty,
            _ => self.clone(),
        }
    }

    pub fn command_in(&self, directory: &Path) -> Command {
        let mut command = Command::new(self.path());
        command
            .current_dir(directory)
            .args(self.arguments(directory));
        command
    }

    pub fn shown(&self) -> String {
        self.path().to_string_lossy().into_owned()
    }

    fn path(&self) -> &OsStr {
        match self {
            Self::XdgTerminalExec(path) => path.as_os_str(),
            Self::Ghostty(name) | Self::Named(name) => name,
        }
    }

    fn arguments(&self, directory: &Path) -> Vec<OsString> {
        match self {
            Self::XdgTerminalExec(_) => {
                vec![Self::XDG_TERMINAL_EXEC_DIRECTORY.argument(directory)]
            }
            Self::Ghostty(_) => vec![
                OsString::from(Self::GHOSTTY_NEW_WINDOW),
                Self::GHOSTTY_DIRECTORY.argument(directory),
            ],
            Self::Named(_) => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ProgramFixture;

    impl ProgramFixture {
        const WORKTREE: &'static str = "/data/ringleader/worktrees/fix-auth";
        const GHOSTTY: &'static str = "/usr/bin/ghostty";
        const LAUNCHER: &'static str = "/usr/bin/xdg-terminal-exec";

        fn arguments_of(program: &TerminalProgram) -> Vec<String> {
            let command = program.command_in(Path::new(Self::WORKTREE));
            command
                .get_args()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect()
        }

        fn launcher() -> TerminalProgram {
            TerminalProgram::XdgTerminalExec(PathBuf::from(Self::LAUNCHER))
        }

        fn launching(terminal: &str) -> TerminalProgram {
            Self::launcher().to_start(|_| Some(OsString::from(terminal)))
        }
    }

    #[test]
    fn a_name_whose_file_is_ghostty_is_ghostty() {
        assert_eq!(
            TerminalProgram::named(OsStr::new(ProgramFixture::GHOSTTY)),
            TerminalProgram::Ghostty(OsString::from(ProgramFixture::GHOSTTY))
        );
    }

    #[test]
    fn a_name_that_only_starts_with_ghostty_is_another_terminal() {
        assert_eq!(
            TerminalProgram::named(OsStr::new("ghostty-nightly")),
            TerminalProgram::Named(OsString::from("ghostty-nightly"))
        );
    }

    #[test]
    fn xdg_terminal_exec_gets_the_directory_as_its_dir_option() {
        assert_eq!(
            ProgramFixture::arguments_of(&ProgramFixture::launcher()),
            [format!("--dir={}", ProgramFixture::WORKTREE)]
        );
    }

    #[test]
    fn ghostty_opens_a_new_window_in_the_directory() {
        let ghostty = TerminalProgram::named(OsStr::new("ghostty"));

        assert_eq!(
            ProgramFixture::arguments_of(&ghostty),
            [
                "+new-window".to_string(),
                format!("--working-directory={}", ProgramFixture::WORKTREE),
            ]
        );
    }

    #[test]
    fn another_named_terminal_gets_no_arguments() {
        let named = TerminalProgram::named(OsStr::new("example-terminal"));

        assert!(ProgramFixture::arguments_of(&named).is_empty());
    }

    #[test]
    fn every_program_starts_in_the_directory() {
        let named = TerminalProgram::named(OsStr::new("example-terminal"));

        assert_eq!(
            named
                .command_in(Path::new(ProgramFixture::WORKTREE))
                .get_current_dir(),
            Some(Path::new(ProgramFixture::WORKTREE))
        );
    }

    #[test]
    fn a_launcher_that_opens_ghostty_gives_way_to_ghostty() {
        assert_eq!(
            ProgramFixture::launching(ProgramFixture::GHOSTTY),
            TerminalProgram::Ghostty(OsString::from(ProgramFixture::GHOSTTY))
        );
    }

    #[test]
    fn a_launcher_that_opens_another_terminal_stays() {
        assert_eq!(
            ProgramFixture::launching("/usr/bin/example-terminal"),
            ProgramFixture::launcher()
        );
    }

    #[test]
    fn a_launcher_that_names_no_terminal_stays() {
        let launcher = ProgramFixture::launcher();

        assert_eq!(launcher.to_start(|_| None), launcher);
    }

    #[test]
    fn a_named_terminal_asks_no_launcher() {
        let named = TerminalProgram::named(OsStr::new("example-terminal"));

        let resolved = named.to_start(|_| panic!("a named terminal has no launcher"));

        assert_eq!(resolved, named);
    }
}
