use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStringExt;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessEnvironment {
    variables: Vec<(OsString, OsString)>,
    terminal: Option<OsString>,
}

impl ProcessEnvironment {
    const TERMINAL_NAME_LIMIT: usize = 256;
    const TMUX_PROGRAM: &'static str = "RINGLEADER_TMUX";
    const GIT_PROGRAM: &'static str = "RINGLEADER_GIT";

    pub fn capture() -> Self {
        Self::new(
            std::env::vars_os().collect(),
            Self::standard_input_terminal(),
        )
    }

    pub fn split_program_paths(&self) -> (ProgramPaths, Self) {
        let program = |variable: &str, fallback: &str| {
            self.get(variable)
                .map(OsStr::to_os_string)
                .unwrap_or_else(|| OsString::from(fallback))
        };
        let paths = ProgramPaths {
            tmux: program(Self::TMUX_PROGRAM, "tmux"),
            git: program(Self::GIT_PROGRAM, "git"),
        };
        let remaining = self.without(&[
            Self::TMUX_PROGRAM.to_string(),
            Self::GIT_PROGRAM.to_string(),
        ]);
        (paths, remaining)
    }

    pub fn new(variables: Vec<(OsString, OsString)>, terminal: Option<OsString>) -> Self {
        Self {
            variables,
            terminal,
        }
    }

    pub fn get(&self, name: &str) -> Option<&OsStr> {
        self.variables
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| value.as_os_str())
    }

    #[cfg(test)]
    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn without(&self, names: &[String]) -> Self {
        Self {
            variables: self
                .variables
                .iter()
                .filter(|(name, _)| !names.iter().any(|removed| name == removed.as_str()))
                .cloned()
                .collect(),
            terminal: self.terminal.clone(),
        }
    }

    pub fn variables(&self) -> &[(OsString, OsString)] {
        &self.variables
    }

    pub fn terminal(&self) -> Option<&OsStr> {
        self.terminal.as_deref()
    }

    fn standard_input_terminal() -> Option<OsString> {
        let mut buffer = vec![0_u8; Self::TERMINAL_NAME_LIMIT];
        // SAFETY: ttyname_r writes at most buffer.len() bytes, including the terminating
        // NUL, into a buffer this function owns, and reads only descriptor 0.
        let status = unsafe { libc::ttyname_r(0, buffer.as_mut_ptr().cast(), buffer.len()) };
        if status != 0 {
            return None;
        }
        let end = buffer.iter().position(|byte| *byte == 0)?;
        buffer.truncate(end);
        Some(OsString::from_vec(buffer))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramPaths {
    tmux: OsString,
    git: OsString,
}

impl ProgramPaths {
    pub fn tmux(&self) -> &OsStr {
        &self.tmux
    }

    pub fn git(&self) -> &OsStr {
        &self.git
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvironmentFixture;

    impl EnvironmentFixture {
        fn with(pairs: &[(&str, &str)]) -> ProcessEnvironment {
            let variables = pairs
                .iter()
                .map(|(name, value)| (OsString::from(name), OsString::from(value)))
                .collect();
            ProcessEnvironment::new(variables, None)
        }
    }

    #[test]
    fn set_program_paths_are_used_and_removed_from_the_snapshot() {
        let environment = EnvironmentFixture::with(&[
            ("RINGLEADER_TMUX", "/nix/store/example-tmux/bin/tmux"),
            ("RINGLEADER_GIT", "/nix/store/example-git/bin/git"),
            ("PATH", "/bin"),
        ]);

        let (paths, remaining) = environment.split_program_paths();

        assert_eq!(paths.tmux(), "/nix/store/example-tmux/bin/tmux");
        assert_eq!(paths.git(), "/nix/store/example-git/bin/git");
        assert!(!remaining.contains("RINGLEADER_TMUX"));
        assert!(!remaining.contains("RINGLEADER_GIT"));
    }

    #[test]
    fn unset_program_paths_fall_back_to_the_search_path() {
        let environment = EnvironmentFixture::with(&[("PATH", "/bin")]);

        let (paths, _) = environment.split_program_paths();

        assert_eq!(
            (paths.tmux(), paths.git()),
            (OsStr::new("tmux"), OsStr::new("git"))
        );
    }

    #[test]
    fn without_removes_the_named_variables() {
        let environment = EnvironmentFixture::with(&[("API_KEY", "secret"), ("PATH", "/bin")]);

        let filtered = environment.without(&["API_KEY".to_string()]);

        assert!(!filtered.contains("API_KEY"));
        assert_eq!(filtered.get("PATH"), Some(OsStr::new("/bin")));
    }
}
