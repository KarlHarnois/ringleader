use std::io;
use std::process::{Command, ExitStatus, Stdio};
use std::thread;

use super::bounded_command::BoundedCommand;

pub struct DetachedCommand {
    command: Command,
}

impl DetachedCommand {
    pub fn new(mut command: Command) -> Self {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        BoundedCommand::start_in_own_session(&mut command);
        Self { command }
    }

    pub fn start(mut self) -> io::Result<()> {
        let mut child = self.command.spawn()?;
        let program = self.command.get_program().to_string_lossy().into_owned();
        thread::spawn(move || Self::warn_on_failure(&program, child.wait()));
        Ok(())
    }

    fn warn_on_failure(program: &str, exit: io::Result<ExitStatus>) {
        match exit {
            Ok(status) if !status.success() => log::warn!("{program} ended with {status}"),
            Ok(_) => {}
            Err(error) => log::warn!("cannot wait for {program}: {error}"),
        }
    }
}
