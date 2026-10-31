use std::cmp::Reverse;
use std::path::{Path, PathBuf};

use crate::domain::{CpuPercent, MemorySize, ProcessFigures};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLoad {
    name: String,
    repo: Option<PathBuf>,
    cpu: CpuPercent,
    memory: MemorySize,
    busiest: Vec<ProcessLoad>,
}

impl SessionLoad {
    const BUSIEST_KEPT: usize = 3;
    const BUSIEST_HEADING: &'static str = "Its busiest processes:";
    const DIAGNOSIS_ASK: &'static str = "Diagnose what causes this load and how to reduce it.";

    pub fn new(name: &str, processes: Vec<ProcessLoad>) -> Self {
        Self {
            name: name.to_string(),
            repo: None,
            cpu: processes.iter().map(ProcessLoad::cpu).sum(),
            memory: processes.iter().map(ProcessLoad::memory).sum(),
            busiest: Self::busiest_of(processes),
        }
    }

    fn busiest_of(mut processes: Vec<ProcessLoad>) -> Vec<ProcessLoad> {
        processes.sort_by_key(|process| Reverse(process.busyness()));
        processes.truncate(Self::BUSIEST_KEPT);
        processes
    }

    pub fn in_repo(self, repo: Option<&Path>) -> Self {
        Self {
            repo: repo.map(Path::to_path_buf),
            ..self
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn repo(&self) -> Option<&Path> {
        self.repo.as_deref()
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

    pub fn diagnosis_prompt(&self) -> String {
        let mut lines = vec![self.summary()];
        if !self.busiest.is_empty() {
            lines.push(Self::BUSIEST_HEADING.to_string());
            lines.extend(self.busiest.iter().map(ProcessLoad::summary));
        }
        lines.push(Self::DIAGNOSIS_ASK.to_string());
        lines.join("\n")
    }

    fn summary(&self) -> String {
        format!(
            "The Ringleader session {} uses {} CPU and {} of memory.",
            self.name, self.cpu, self.memory
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessLoad {
    pid: u32,
    program: String,
    cpu: CpuPercent,
    memory: MemorySize,
}

impl ProcessLoad {
    pub fn new(process: &ProcessFigures, cpu: CpuPercent) -> Self {
        Self {
            pid: process.pid(),
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

    fn summary(&self) -> String {
        format!(
            "- {} (pid {}): {} CPU, {}",
            self.program, self.pid, self.cpu, self.memory
        )
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
            Self::load(Self::figures(1, program, mebibytes), cpu_milliseconds)
        }

        fn figures(pid: u32, program: &str, mebibytes: u64) -> ProcessFigures {
            ProcessFigures::new(pid, 0, program)
                .with_resident_memory(MemorySize::of_bytes(mebibytes * Self::MEBIBYTE))
        }

        fn load(figures: ProcessFigures, cpu_milliseconds: u64) -> ProcessLoad {
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
    fn the_diagnosis_prompt_holds_the_figures_of_the_session_and_its_busiest_processes() {
        let load = SessionLoad::new(
            "fix-auth",
            vec![
                LoadFixture::load(LoadFixture::figures(4242, "node", 512), 900),
                LoadFixture::load(LoadFixture::figures(4243, "sh", 4), 50),
            ],
        );

        assert_eq!(
            load.diagnosis_prompt(),
            [
                "The Ringleader session fix-auth uses 95.0% CPU and 516 MiB of memory.",
                "Its busiest processes:",
                "- node (pid 4242): 90.0% CPU, 512 MiB",
                "- sh (pid 4243): 5.0% CPU, 4.0 MiB",
                "Diagnose what causes this load and how to reduce it.",
            ]
            .join("\n")
        );
    }

    #[test]
    fn a_session_without_processes_asks_for_a_diagnosis_of_its_totals_alone() {
        let load = SessionLoad::new("fix-auth", Vec::new());

        assert_eq!(
            load.diagnosis_prompt(),
            "The Ringleader session fix-auth uses 0.0% CPU and 0 B of memory.\nDiagnose what causes this load and how to reduce it."
        );
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
