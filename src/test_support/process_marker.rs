use std::ffi::OsString;
use std::path::Path;
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::adapters::HostProcessTable;
use crate::adapters::host::process_table::{ProcessScope, ProcessTable};

static MARKER_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub struct ProcessMarker {
    value: String,
}

impl ProcessMarker {
    pub const NAME: &'static str = "RINGLEADER_TEST_PROCESS";
    const SLEEP_SECONDS: &'static str = "30";

    pub fn new() -> Self {
        let sequence = MARKER_COUNTER.fetch_add(1, Ordering::Relaxed);
        Self {
            value: format!("{}-{sequence}", std::process::id()),
        }
    }

    pub fn variable(&self) -> (OsString, OsString) {
        (OsString::from(Self::NAME), OsString::from(&self.value))
    }

    pub fn table(&self) -> ProcessTable {
        ProcessTable::within(Arc::new(self.clone()))
    }

    pub fn sleeping_child(&self) -> Child {
        self.sleep_command().spawn().expect("a marked sleep starts")
    }

    pub fn sleeping_child_in(&self, directory: &Path) -> Child {
        self.sleep_command()
            .current_dir(directory)
            .spawn()
            .expect("a marked sleep starts in the directory")
    }

    fn sleep_command(&self) -> Command {
        let mut command = Command::new("sleep");
        command.arg(Self::SLEEP_SECONDS).envs([self.variable()]);
        command
    }

    fn entry(&self) -> String {
        format!("{}={}", Self::NAME, self.value)
    }
}

impl ProcessScope for ProcessMarker {
    fn lists(&self, pid: u32) -> bool {
        HostProcessTable::environment(pid).contains(&self.entry())
    }
}
