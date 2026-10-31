use std::time::Duration;

use super::memory_size::MemorySize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessFigures {
    pid: u32,
    parent: u32,
    program: String,
    cpu_time: Duration,
    resident_memory: MemorySize,
}

impl ProcessFigures {
    pub fn new(pid: u32, parent: u32, program: &str) -> Self {
        Self {
            pid,
            parent,
            program: program.to_string(),
            cpu_time: Duration::ZERO,
            resident_memory: MemorySize::default(),
        }
    }

    pub fn with_cpu_time(self, cpu_time: Duration) -> Self {
        Self { cpu_time, ..self }
    }

    pub fn with_resident_memory(self, resident_memory: MemorySize) -> Self {
        Self {
            resident_memory,
            ..self
        }
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn parent(&self) -> u32 {
        self.parent
    }

    pub fn program(&self) -> &str {
        &self.program
    }

    pub fn resident_memory(&self) -> MemorySize {
        self.resident_memory
    }

    pub fn cpu_time_since(&self, earlier: Option<&ProcessFigures>) -> Duration {
        let spent_before = earlier.map_or(Duration::ZERO, |earlier| earlier.cpu_time);
        self.cpu_time.saturating_sub(spent_before)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FiguresFixture;

    impl FiguresFixture {
        fn spent(milliseconds: u64) -> ProcessFigures {
            ProcessFigures::new(4242, 1, "example-program")
                .with_cpu_time(Duration::from_millis(milliseconds))
        }
    }

    #[test]
    fn the_cpu_time_since_an_earlier_sample_is_the_time_spent_in_between() {
        let earlier = FiguresFixture::spent(700);

        let spent = FiguresFixture::spent(1000).cpu_time_since(Some(&earlier));

        assert_eq!(spent, Duration::from_millis(300));
    }

    #[test]
    fn a_process_the_earlier_sample_lacks_spent_all_its_cpu_time_since() {
        let spent = FiguresFixture::spent(400).cpu_time_since(None);

        assert_eq!(spent, Duration::from_millis(400));
    }

    #[test]
    fn a_reused_pid_that_spent_less_than_its_predecessor_spent_nothing() {
        let predecessor = FiguresFixture::spent(9000);

        let spent = FiguresFixture::spent(100).cpu_time_since(Some(&predecessor));

        assert_eq!(spent, Duration::ZERO);
    }
}
