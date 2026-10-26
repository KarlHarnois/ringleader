use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Command;

use crate::adapters::git::git_invocation::GitInvocation;

#[derive(Debug, Clone)]
pub struct GhProgram {
    path: OsString,
}

impl GhProgram {
    const REPOSITORY_OVERRIDE: &'static str = "GH_REPO";

    pub fn new(path: &OsStr) -> Self {
        Self {
            path: path.to_os_string(),
        }
    }

    pub fn in_repo(&self, repo: &Path) -> Command {
        let mut command = Command::new(&self.path);
        command
            .current_dir(repo)
            .env_remove(Self::REPOSITORY_OVERRIDE)
            .env("GH_PROMPT_DISABLED", "1")
            .env("NO_COLOR", "1");
        for name in GitInvocation::REPOSITORY_LOCAL_VARIABLES {
            command.env_remove(name);
        }
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_gh_call_ignores_inherited_repository_overrides() {
        let command = GhProgram::new(OsStr::new("gh")).in_repo(Path::new("/repos/example"));
        let removed: Vec<&OsStr> = command
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name)
            .collect();

        assert!(removed.contains(&OsStr::new("GH_REPO")));
        assert!(removed.contains(&OsStr::new("GIT_DIR")));
        assert!(removed.contains(&OsStr::new("GIT_WORK_TREE")));
    }
}
