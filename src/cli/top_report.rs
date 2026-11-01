use std::iter::once;

use crate::domain::{CpuPercent, MemorySize, ProcessLoad, SessionLoad};

pub struct TopReport<'a> {
    loads: &'a [SessionLoad],
}

impl<'a> TopReport<'a> {
    const LABEL_WIDTH: usize = 24;
    const CPU_WIDTH: usize = 7;
    const MEMORY_WIDTH: usize = 9;
    const PROCESS_INDENT: &'static str = "  ";

    pub fn new(loads: &'a [SessionLoad]) -> Self {
        Self { loads }
    }

    pub fn lines(&self) -> Vec<String> {
        self.loads.iter().flat_map(Self::session_lines).collect()
    }

    fn session_lines(load: &SessionLoad) -> Vec<String> {
        let processes = load.busiest().iter().map(Self::process_line);
        once(Self::line(load.name(), load.cpu(), load.memory()))
            .chain(processes)
            .collect()
    }

    fn process_line(process: &ProcessLoad) -> String {
        let label = format!("{}{}", Self::PROCESS_INDENT, process.program());
        Self::line(&label, process.cpu(), process.memory())
    }

    fn line(label: &str, cpu: CpuPercent, memory: MemorySize) -> String {
        format!(
            "{label:<label_width$} {cpu:>cpu_width$} {memory:>memory_width$}",
            label_width = Self::LABEL_WIDTH,
            cpu_width = Self::CPU_WIDTH,
            memory_width = Self::MEMORY_WIDTH
        )
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::domain::ProcessFigures;
    use crate::test_support::ProcessFiguresFixture;

    struct ReportFixture;

    impl ReportFixture {
        fn process(program: &str, cpu_milliseconds: u64, mebibytes: u64) -> ProcessLoad {
            let figures = ProcessFigures::new(1, 0, program)
                .with_resident_memory(ProcessFiguresFixture::mebibytes(mebibytes));
            let cpu = CpuPercent::of(
                Duration::from_millis(cpu_milliseconds),
                Duration::from_secs(1),
            );
            ProcessLoad::new(&figures, cpu)
        }
    }

    #[test]
    fn a_session_line_is_followed_by_its_busiest_processes_indented() {
        let load = SessionLoad::new(
            "fix-auth",
            vec![
                ReportFixture::process("sh", 50, 4),
                ReportFixture::process("node", 900, 512),
            ],
        );

        assert_eq!(
            TopReport::new(&[load]).lines(),
            [
                "fix-auth                   95.0%   516 MiB",
                "  node                     90.0%   512 MiB",
                "  sh                        5.0%   4.0 MiB",
            ]
        );
    }

    #[test]
    fn no_running_session_prints_nothing() {
        assert!(TopReport::new(&[]).lines().is_empty());
    }
}
