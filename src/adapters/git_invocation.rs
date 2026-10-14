use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::bounded_command::Call;
use super::deadlines::CallKind;

pub struct GitInvocation {
    subcommand: String,
    location: GitLocation,
    args: Vec<OsString>,
    kind: CallKind,
}

impl GitInvocation {
    pub const REPOSITORY_LOCAL_VARIABLES: [&'static str; 13] = [
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG",
        "GIT_OBJECT_DIRECTORY",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_IMPLICIT_WORK_TREE",
        "GIT_GRAFT_FILE",
        "GIT_INDEX_FILE",
        "GIT_NO_REPLACE_OBJECTS",
        "GIT_REPLACE_REF_BASE",
        "GIT_PREFIX",
        "GIT_SHALLOW_FILE",
        "GIT_COMMON_DIR",
    ];

    pub fn in_repo(repo: &Path, subcommand: &str) -> Self {
        Self::at(GitLocation::Directory(repo.to_path_buf(), None), subcommand)
    }

    pub fn in_worktree(worktree: &Path, subcommand: &str) -> Self {
        let ceiling = worktree.parent().map(Path::to_path_buf);
        Self::at(
            GitLocation::Directory(worktree.to_path_buf(), ceiling),
            subcommand,
        )
    }

    pub fn in_git_dir(git_dir: &Path, work_tree: &Path, subcommand: &str) -> Self {
        let location = GitLocation::GitDir(git_dir.to_path_buf(), work_tree.to_path_buf());
        Self::at(location, subcommand)
    }

    pub fn anywhere(subcommand: &str) -> Self {
        Self::at(GitLocation::Anywhere, subcommand)
    }

    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.args.push(arg.as_ref().to_os_string());
        self
    }

    pub fn args<I, A>(self, args: I) -> Self
    where
        I: IntoIterator<Item = A>,
        A: AsRef<OsStr>,
    {
        args.into_iter().fold(self, Self::arg)
    }

    pub fn writing(self) -> Self {
        Self {
            kind: CallKind::GitWrite,
            ..self
        }
    }

    pub fn diffing(self) -> Self {
        Self {
            kind: CallKind::GitDiff,
            ..self
        }
    }

    pub fn subcommand(&self) -> &str {
        &self.subcommand
    }

    pub fn call(&self) -> Call {
        Call::new(&format!("git {}", self.subcommand), self.kind)
    }

    pub fn command(&self, program: &OsStr, overrides: &[(OsString, OsString)]) -> Command {
        let mut command = Command::new(program);
        command.envs(overrides.iter().map(|(name, value)| (name, value)));
        for name in Self::REPOSITORY_LOCAL_VARIABLES {
            command.env_remove(name);
        }
        command.env("GIT_TERMINAL_PROMPT", "0");
        if self.kind != CallKind::GitWrite {
            command.env("GIT_OPTIONAL_LOCKS", "0");
            command.args(["-c", "core.fsmonitor=false"]);
        }
        self.location.apply(&mut command);
        command.arg(&self.subcommand).args(&self.args);
        command
    }

    fn at(location: GitLocation, subcommand: &str) -> Self {
        Self {
            subcommand: subcommand.to_string(),
            location,
            args: Vec::new(),
            kind: CallKind::GitRead,
        }
    }
}

enum GitLocation {
    Directory(PathBuf, Option<PathBuf>),
    GitDir(PathBuf, PathBuf),
    Anywhere,
}

impl GitLocation {
    fn apply(&self, command: &mut Command) {
        match self {
            GitLocation::Directory(directory, ceiling) => {
                if let Some(ceiling) = ceiling {
                    command.env("GIT_CEILING_DIRECTORIES", ceiling);
                }
                command.arg("-C").arg(directory);
            }
            GitLocation::GitDir(git_dir, work_tree) => {
                command
                    .arg(Self::option("--git-dir=", git_dir))
                    .arg(Self::option("--work-tree=", work_tree));
            }
            GitLocation::Anywhere => {}
        }
    }

    fn option(name: &str, path: &Path) -> OsString {
        let mut option = OsString::from(name);
        option.push(path);
        option
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TERMINAL_PROMPT: &str = "GIT_TERMINAL_PROMPT";

    #[test]
    fn every_call_kind_disables_terminal_prompts_despite_overrides() {
        let allowing_prompts = [(OsString::from(TERMINAL_PROMPT), OsString::from("1"))];
        let every_call_kind = [
            GitInvocation::anywhere("status"),
            GitInvocation::anywhere("diff").diffing(),
            GitInvocation::anywhere("worktree").writing(),
        ];

        for invocation in every_call_kind {
            let command = invocation.command(OsStr::new("git"), &allowing_prompts);

            let terminal_prompt = command
                .get_envs()
                .find(|(name, _)| *name == TERMINAL_PROMPT)
                .and_then(|(_, value)| value);
            let subcommand = invocation.subcommand();
            assert_eq!(terminal_prompt, Some(OsStr::new("0")), "git {subcommand}");
        }
    }
}
