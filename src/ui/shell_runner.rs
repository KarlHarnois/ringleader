use std::io;
use std::process::Command;

use crate::application::EphemeralShell;

pub trait ShellRunner {
    fn run(&mut self, shell: &EphemeralShell) -> io::Result<()>;
}

pub struct ChildShell;

impl ShellRunner for ChildShell {
    fn run(&mut self, shell: &EphemeralShell) -> io::Result<()> {
        Command::new(shell.program())
            .current_dir(shell.worktree())
            .status()
            .map(drop)
    }
}
