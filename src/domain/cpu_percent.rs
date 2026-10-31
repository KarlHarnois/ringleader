use std::fmt;
use std::iter::Sum;
use std::time::Duration;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct CpuPercent {
    tenths: u64,
}

impl CpuPercent {
    const TENTHS_PER_PERCENT: u64 = 10;
    const TENTHS_OF_ONE_CORE: u128 = 1000;

    pub fn of(cpu_time: Duration, elapsed: Duration) -> Self {
        let elapsed = elapsed.as_nanos();
        if elapsed == 0 {
            return Self::default();
        }
        let tenths = (cpu_time.as_nanos() * Self::TENTHS_OF_ONE_CORE + elapsed / 2) / elapsed;
        Self {
            tenths: u64::try_from(tenths).unwrap_or(u64::MAX),
        }
    }

    fn label(self) -> String {
        let whole = self.tenths / Self::TENTHS_PER_PERCENT;
        let tenth = self.tenths % Self::TENTHS_PER_PERCENT;
        format!("{whole}.{tenth}%")
    }
}

impl Sum for CpuPercent {
    fn sum<I: Iterator<Item = Self>>(shares: I) -> Self {
        let tenths = shares.fold(0, |total: u64, share| total.saturating_add(share.tenths));
        Self { tenths }
    }
}

impl fmt::Display for CpuPercent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.pad(&self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ShareFixture;

    impl ShareFixture {
        fn milliseconds(cpu: u64, elapsed: u64) -> CpuPercent {
            CpuPercent::of(Duration::from_millis(cpu), Duration::from_millis(elapsed))
        }
    }

    #[test]
    fn the_cpu_percentage_is_the_cpu_time_divided_by_the_elapsed_time() {
        assert_eq!(ShareFixture::milliseconds(250, 1000).to_string(), "25.0%");
    }

    #[test]
    fn a_process_on_two_cores_uses_more_than_one_hundred_percent() {
        assert_eq!(ShareFixture::milliseconds(3000, 2000).to_string(), "150.0%");
    }

    #[test]
    fn the_cpu_percentage_rounds_to_the_nearest_tenth() {
        assert_eq!(ShareFixture::milliseconds(1, 3).to_string(), "33.3%");
    }

    #[test]
    fn no_elapsed_time_counts_as_no_cpu() {
        assert_eq!(ShareFixture::milliseconds(500, 0), CpuPercent::default());
    }

    #[test]
    fn percentages_add_up() {
        let total: CpuPercent = [
            ShareFixture::milliseconds(100, 1000),
            ShareFixture::milliseconds(250, 1000),
        ]
        .into_iter()
        .sum();

        assert_eq!(total.to_string(), "35.0%");
    }

    #[test]
    fn a_percentage_pads_to_the_width_it_is_given() {
        assert_eq!(
            format!("{:>7}", ShareFixture::milliseconds(50, 1000)),
            "   5.0%"
        );
    }
}
