use std::time::Duration;

use crate::domain::{MemorySize, ProcessFigures, ProcessSample};

pub struct ProcessFiguresFixture;

impl ProcessFiguresFixture {
    const PROGRAM: &'static str = "example-program";
    const MEBIBYTE: u64 = 1024 * 1024;

    pub fn process(pid: u32, parent: u32, cpu_milliseconds: u64) -> ProcessFigures {
        ProcessFigures::new(pid, parent, Self::PROGRAM)
            .with_cpu_time(Duration::from_millis(cpu_milliseconds))
    }

    pub fn sample_at(seconds: u64, processes: Vec<ProcessFigures>) -> ProcessSample {
        ProcessSample::new(Duration::from_secs(seconds), processes)
    }

    pub fn mebibytes(count: u64) -> MemorySize {
        MemorySize::of_bytes(count * Self::MEBIBYTE)
    }
}
