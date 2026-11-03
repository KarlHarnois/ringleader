use std::path::Path;
use std::process::Command;

use crate::adapters::child_process::bounded_command::Call;
use crate::adapters::child_process::deadlines::CallKind;
use crate::adapters::{BoundedCommand, Deadlines};

pub struct CpReflinkTree {
    bounded: BoundedCommand,
}

impl CpReflinkTree {
    const PROGRAM: &'static str = "cp";
    const SUBCOMMAND: &'static str = "cp --reflink";
    const CLONE_PRESERVING_BUILD_TIMES: [&'static str; 3] =
        ["-R", "--reflink=always", "--preserve=mode,timestamps"];

    pub fn new() -> Self {
        Self {
            bounded: BoundedCommand::new(Deadlines::standard()),
        }
    }

    pub fn clone_tree(&self, source: &Path, destination: &Path) -> Result<(), String> {
        let mut command = Command::new(Self::PROGRAM);
        command
            .args(Self::CLONE_PRESERVING_BUILD_TIMES)
            .arg(source)
            .arg(destination);
        let call = Call::new(Self::SUBCOMMAND, CallKind::BuildCacheCopy);
        let output = self
            .bounded
            .run(command, &call)
            .map_err(|error| error.to_string())?;
        match output.succeeded() {
            true => Ok(()),
            false => Err(output.stderr_text().trim().to_string()),
        }
    }
}
