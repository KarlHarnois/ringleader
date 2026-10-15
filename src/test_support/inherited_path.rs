use std::ffi::OsString;
use std::path::PathBuf;

pub struct InheritedPath;

impl InheritedPath {
    #[expect(
        clippy::disallowed_methods,
        reason = "tests run tmux, git, and direnv from the PATH they inherit"
    )]
    pub fn value() -> OsString {
        std::env::var_os("PATH").unwrap_or_default()
    }

    pub fn program(name: &str) -> Option<PathBuf> {
        std::env::split_paths(&Self::value())
            .map(|directory| directory.join(name))
            .find(|candidate| candidate.is_file())
    }
}
