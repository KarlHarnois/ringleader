use std::path::Path;

use crate::application::ports::ToolTempDirectory;

pub struct ToolTempName;

impl ToolTempName {
    const SHELL_PREFIX: &'static str = "nix-shell.";
    const SHELL_SUFFIX_LENGTH: usize = 6;
    const LAUNCHER_PREFIX: &'static str = "nix-develop-";
    const LAUNCHER_SEPARATOR: char = '-';

    pub fn recognize(path: &Path) -> Option<ToolTempDirectory> {
        let name = path.file_name()?.to_str()?;
        if Self::is_shell(name) {
            return Some(ToolTempDirectory::ownerless(path.to_path_buf()));
        }
        let owner = Self::launcher_owner(name)?;
        Some(ToolTempDirectory::owned(path.to_path_buf(), owner))
    }

    fn is_shell(name: &str) -> bool {
        name.strip_prefix(Self::SHELL_PREFIX).is_some_and(|suffix| {
            suffix.len() == Self::SHELL_SUFFIX_LENGTH
                && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
    }

    fn launcher_owner(name: &str) -> Option<u32> {
        let (pid, counter) = name
            .strip_prefix(Self::LAUNCHER_PREFIX)?
            .split_once(Self::LAUNCHER_SEPARATOR)?;
        counter.parse::<u64>().ok()?;
        pid.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    struct NameFixture;

    impl NameFixture {
        const SCRATCH: &'static str = "/cache/ringleader/tmp/8718940d";

        fn recognized(name: &str) -> Option<ToolTempDirectory> {
            ToolTempName::recognize(&Self::path(name))
        }

        fn path(name: &str) -> PathBuf {
            Path::new(Self::SCRATCH).join(name)
        }
    }

    #[test]
    fn a_nix_shell_folder_is_an_ownerless_tool_temp_directory() {
        assert_eq!(
            NameFixture::recognized("nix-shell.Ab12Cd"),
            Some(ToolTempDirectory::ownerless(NameFixture::path(
                "nix-shell.Ab12Cd"
            )))
        );
    }

    #[test]
    fn a_nix_develop_folder_is_owned_by_the_pid_in_its_name() {
        assert_eq!(
            NameFixture::recognized("nix-develop-4100-2448752463"),
            Some(ToolTempDirectory::owned(
                NameFixture::path("nix-develop-4100-2448752463"),
                4100
            ))
        );
    }

    #[test]
    fn other_names_are_not_tool_temp_directories() {
        for name in [
            "claude-1000",
            "nix-shell.nix",
            "nix-shell.Ab12Cd.bak",
            "nix-develop-4100",
            "nix-develop-abc-1",
            "nix-develop-4100-x",
            "proc-macro-srv694f-0",
        ] {
            assert_eq!(NameFixture::recognized(name), None, "{name}");
        }
    }
}
