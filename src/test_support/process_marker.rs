use std::ffi::OsString;
use std::fs;
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

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
        Command::new("sleep")
            .arg(Self::SLEEP_SECONDS)
            .envs([self.variable()])
            .spawn()
            .expect("a marked sleep starts")
    }

    fn entry(&self) -> Vec<u8> {
        format!("{}={}", Self::NAME, self.value).into_bytes()
    }
}

impl ProcessScope for ProcessMarker {
    fn lists(&self, pid: u32) -> bool {
        let entry = self.entry();
        fs::read(format!("/proc/{pid}/environ")).is_ok_and(|environ| {
            environ
                .split(|byte| *byte == 0)
                .any(|candidate| candidate == entry)
        })
    }
}
