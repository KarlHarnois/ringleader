use std::cmp::Reverse;

use crate::domain::{CpuPercent, MemorySize, ProcessFigures};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLoad {
    name: String,
    cpu: CpuPercent,
    memory: MemorySize,
    busiest: Vec<ProcessLoad>,
}

impl SessionLoad {
    const BUSIEST_KEPT: usize = 3;

    pub fn new(name: &str, mut processes: Vec<ProcessLoad>) -> Self {
        let cpu = processes.iter().map(ProcessLoad::cpu).sum();
        let memory = processes.iter().map(ProcessLoad::memory).sum();
        processes.sort_by_key(|process| Reverse(process.busyness()));
        processes.truncate(Self::BUSIEST_KEPT);
        Self {
            name: name.to_string(),
            cpu,
            memory,
            busiest: processes,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn cpu(&self) -> CpuPercent {
        self.cpu
    }

    pub fn memory(&self) -> MemorySize {
        self.memory
    }

    pub fn busiest(&self) -> &[ProcessLoad] {
        &self.busiest
    }

    pub fn busyness(&self) -> (CpuPercent, MemorySize) {
        (self.cpu, self.memory)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessLoad {
    program: String,
    cpu: CpuPercent,
    memory: MemorySize,
}

impl ProcessLoad {
    pub fn new(process: &ProcessFigures, cpu: CpuPercent) -> Self {
        Self {
            program: process.program().to_string(),
            cpu,
            memory: process.resident_memory(),
        }
    }

    pub fn program(&self) -> &str {
        &self.program
    }

    pub fn cpu(&self) -> CpuPercent {
        self.cpu
    }

    pub fn memory(&self) -> MemorySize {
        self.memory
    }

    fn busyness(&self) -> (CpuPercent, MemorySize) {
        (self.cpu, self.memory)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    struct LoadFixture;

    impl LoadFixture {
        const MEBIBYTE: u64 = 1024 * 1024;

        fn process(program: &str, cpu_milliseconds: u64, mebibytes: u64) -> ProcessLoad {
            let figures = ProcessFigures::new(1, 0, program)
                .with_resident_memory(MemorySize::of_bytes(mebibytes * Self::MEBIBYTE));
            let cpu = CpuPercent::of(
                Duration::from_millis(cpu_milliseconds),
                Duration::from_secs(1),
            );
            ProcessLoad::new(&figures, cpu)
        }

        fn programs(load: &SessionLoad) -> Vec<&str> {
            load.busiest().iter().map(ProcessLoad::program).collect()
        }
    }

    #[test]
    fn a_session_totals_the_cpu_of_all_its_processes() {
        let load = SessionLoad::new(
            "fix-auth",
            vec![
                LoadFixture::process("node", 300, 1),
                LoadFixture::process("sh", 50, 1),
            ],
        );

        assert_eq!(load.cpu().to_string(), "35.0%");
    }

    #[test]
    fn a_session_totals_the_memory_of_all_its_processes() {
        let load = SessionLoad::new(
            "fix-auth",
            vec![
                LoadFixture::process("node", 0, 3),
                LoadFixture::process("sh", 0, 2),
            ],
        );

        assert_eq!(load.memory().to_string(), "5.0 MiB");
    }

    #[test]
    fn a_session_keeps_its_three_busiest_processes_busiest_first() {
        let load = SessionLoad::new(
            "fix-auth",
            vec![
                LoadFixture::process("sh", 10, 1),
                LoadFixture::process("node", 900, 1),
                LoadFixture::process("cat", 1, 1),
                LoadFixture::process("cargo", 400, 1),
            ],
        );

        assert_eq!(LoadFixture::programs(&load), ["node", "cargo", "sh"]);
    }

    #[test]
    fn processes_that_use_the_same_cpu_rank_by_memory() {
        let load = SessionLoad::new(
            "fix-auth",
            vec![
                LoadFixture::process("sh", 0, 1),
                LoadFixture::process("node", 0, 300),
            ],
        );

        assert_eq!(LoadFixture::programs(&load), ["node", "sh"]);
    }
}
