use std::ffi::OsString;
use std::path::Path;
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::adapters::HostProcessTable;
use crate::adapters::host::process_table::{ProcessScope, ProcessTable};
use crate::test_support::Deadline;

static MARKER_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub struct ProcessMarker {
    value: String,
}

impl ProcessMarker {
    pub const NAME: &'static str = "RINGLEADER_TEST_PROCESS";
    const SLEEP_SECONDS: &'static str = "30";
    const OTHER_TEMP_VARIABLES: [&'static str; 4] = ["TMP", "TEMP", "TEMPDIR", "NIX_BUILD_TOP"];

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
        self.started(&mut self.sleep_command())
    }

    pub fn sleeping_child_in(&self, directory: &Path) -> Child {
        self.started(self.sleep_command().current_dir(directory))
    }

    pub fn sleeping_child_with_temp(&self, temp_directory: &Path) -> Child {
        let mut command = self.sleep_command();
        for inherited in Self::OTHER_TEMP_VARIABLES {
            command.env_remove(inherited);
        }
        self.started(command.env("TMPDIR", temp_directory))
    }

    fn started(&self, command: &mut Command) -> Child {
        let child = command.spawn().expect("a marked sleep starts");
        let started = Deadline::for_background_work().wait_until(|| self.lists(child.id()));
        assert!(started, "the marked sleep never ran");
        child
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
