use std::ffi::OsString;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathOption {
    name: &'static str,
}

impl PathOption {
    pub const fn new(name: &'static str) -> Self {
        Self { name }
    }

    pub fn argument(&self, path: &Path) -> OsString {
        let mut argument = OsString::from(self.name);
        argument.push(path);
        argument
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_follows_the_option_in_one_argument() {
        let directory = PathOption::new("--dir=");

        assert_eq!(
            directory.argument(Path::new("/data/ringleader/worktrees/fix-auth")),
            "--dir=/data/ringleader/worktrees/fix-auth"
        );
    }
}
