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
    use std::ops::RangeInclusive;
    use std::process::{Child, Command};
    use std::time::Duration;

    use super::*;
    use crate::domain::{MemorySize, ProcessFigures};
    use crate::test_support::{Deadline, ProcessMarker};

    struct MeterFixture;

    impl MeterFixture {
        const BUSY_LOOP: &'static str = "while :; do :; done";
        const CPU_TIME_TARGET: Duration = Duration::from_millis(100);

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

        fn memory_readings_once_they_agree(meter: &SystemProcessMeter, pid: u32) -> MemoryReadings {
            let mut readings = MemoryReadings::default();
            Deadline::for_background_work().wait_until(|| {
                readings = MemoryReadings::take(meter, pid);
                readings.agree()
            });
            readings
        }

        fn end(mut child: Child) {
            child.kill().unwrap();
            child.wait().unwrap();
        }
    }

    #[derive(Debug, Default)]
    struct MemoryReadings {
        sampled: Option<MemorySize>,
        kibibytes_from_ps: Option<u64>,
    }

    impl MemoryReadings {
        const BYTES_PER_KIBIBYTE: u64 = 1024;

        fn take(meter: &SystemProcessMeter, pid: u32) -> Self {
            Self {
                sampled: meter
                    .sample()
                    .unwrap()
                    .figures_of(pid)
                    .map(ProcessFigures::resident_memory),
                kibibytes_from_ps: Self::ask_ps_for_kibibytes(pid),
            }
        }

        fn agree(&self) -> bool {
            self.sampled
                .zip(self.kibibytes_from_ps)
                .is_some_and(|(sampled, kibibytes)| {
                    Self::sizes_within_half_of(kibibytes).contains(&sampled)
                })
        }

        fn ask_ps_for_kibibytes(pid: u32) -> Option<u64> {
            let output = Command::new("ps")
                .args(["-o", "rss=", "-p", &pid.to_string()])
                .output()
                .ok()?;
            String::from_utf8_lossy(&output.stdout).trim().parse().ok()
        }

        fn sizes_within_half_of(kibibytes: u64) -> RangeInclusive<MemorySize> {
            let bytes = kibibytes * Self::BYTES_PER_KIBIBYTE;
            let half = bytes / 2;
            MemorySize::of_bytes(bytes - half)..=MemorySize::of_bytes(bytes + half)
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
    fn a_busy_child_is_sampled_with_cpu_time_that_keeps_pace_with_the_clock() {
        let marker = ProcessMarker::new();
        let started = Instant::now();
        let child = MeterFixture::busy_child(&marker);
        let meter = SystemProcessMeter::new(marker.table());

        let figures = MeterFixture::figures_once(&meter, child.id(), |figures| {
            figures.cpu_time_since(None) >= MeterFixture::CPU_TIME_TARGET
        });
        let ran_for = started.elapsed();
        MeterFixture::end(child);

        let spent = figures.map(|figures| figures.cpu_time_since(None));
        assert!(
            spent.is_some_and(|spent| (MeterFixture::CPU_TIME_TARGET..=ran_for).contains(&spent)),
            "a child busy for {ran_for:?} spent {spent:?} of CPU time"
        );
    }

    #[test]
    fn a_sleeping_child_is_sampled_with_the_resident_memory_that_ps_reports() {
        let marker = ProcessMarker::new();
        let child = marker.sleeping_child();
        let meter = SystemProcessMeter::new(marker.table());

        let readings = MeterFixture::memory_readings_once_they_agree(&meter, child.id());
        MeterFixture::end(child);

        assert!(readings.agree(), "{readings:?}");
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
