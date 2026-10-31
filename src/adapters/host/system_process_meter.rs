use std::time::Instant;

use super::process_table::ProcessTable;
use crate::application::ports::{ProcessMeter, ProcessMeterError};
use crate::domain::ProcessSample;

pub struct SystemProcessMeter {
    processes: ProcessTable,
    origin: Instant,
}

impl SystemProcessMeter {
    pub fn new(processes: ProcessTable) -> Self {
        Self {
            processes,
            origin: Instant::now(),
        }
    }
}

impl ProcessMeter for SystemProcessMeter {
    fn sample(&self) -> Result<ProcessSample, ProcessMeterError> {
        let processes =
            self.processes
                .figures()
                .map_err(|error| ProcessMeterError::Unreadable {
                    detail: error.to_string(),
                })?;
        Ok(ProcessSample::new(self.origin.elapsed(), processes))
    }
}

#[cfg(test)]
mod tests {
    use std::process::{Child, Command};
    use std::time::Duration;

    use super::*;
    use crate::domain::ProcessFigures;
    use crate::test_support::{Deadline, ProcessMarker};

    struct MeterFixture;

    impl MeterFixture {
        const BUSY_LOOP: &'static str = "while :; do :; done";

        fn busy_child(marker: &ProcessMarker) -> Child {
            Command::new("sh")
                .args(["-c", Self::BUSY_LOOP])
                .envs([marker.variable()])
                .spawn()
                .expect("a marked busy loop starts")
        }

        fn figures_once(
            meter: &SystemProcessMeter,
            pid: u32,
            done: impl Fn(&ProcessFigures) -> bool,
        ) -> Option<ProcessFigures> {
            let mut figures = None;
            Deadline::for_background_work().wait_until(|| {
                figures = meter.sample().unwrap().figures_of(pid).cloned();
                figures.as_ref().is_some_and(&done)
            });
            figures
        }

        fn end(mut child: Child) {
            child.kill().unwrap();
            child.wait().unwrap();
        }
    }

    #[test]
    fn a_marked_child_is_sampled_with_its_parent_and_program() {
        let marker = ProcessMarker::new();
        let child = marker.sleeping_child();
        let meter = SystemProcessMeter::new(marker.table());

        let figures =
            MeterFixture::figures_once(&meter, child.id(), |figures| figures.program() == "sleep");
        MeterFixture::end(child);

        assert_eq!(figures.unwrap().parent(), std::process::id());
    }

    #[test]
    fn a_busy_child_is_sampled_with_the_cpu_time_it_spent() {
        let marker = ProcessMarker::new();
        let child = MeterFixture::busy_child(&marker);
        let meter = SystemProcessMeter::new(marker.table());

        let figures = MeterFixture::figures_once(&meter, child.id(), |figures| {
            figures.cpu_time_since(None) > Duration::ZERO
        });
        MeterFixture::end(child);

        assert!(figures.is_some());
    }

    #[test]
    fn a_sample_lists_only_the_processes_of_its_table() {
        let meter = SystemProcessMeter::new(ProcessMarker::new().table());

        let sample = meter.sample().unwrap();

        assert_eq!(sample.figures_of(std::process::id()), None);
    }

    #[test]
    fn a_later_sample_is_stamped_later() {
        let meter = SystemProcessMeter::new(ProcessMarker::new().table());
        let earlier = meter.sample().unwrap();
        std::thread::sleep(Duration::from_millis(5));

        let later = meter.sample().unwrap();

        assert!(later.time_since(&earlier) > Duration::ZERO);
    }
}
