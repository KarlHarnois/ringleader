use std::fmt;
use std::iter::Sum;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct MemorySize {
    bytes: u64,
}

impl MemorySize {
    const STEP: u128 = 1024;
    const UNITS: [&'static str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    const TENTHS_PER_UNIT: u128 = 10;
    const TENTHS_SHOWN_UNDER: u128 = 100;

    pub fn of_bytes(bytes: u64) -> Self {
        Self { bytes }
    }

    fn label(self) -> String {
        let Some((unit, unit_bytes)) = self.unit() else {
            return format!("{} B", self.bytes);
        };
        let bytes = u128::from(self.bytes);
        let tenths = Self::rounded(bytes * Self::TENTHS_PER_UNIT, unit_bytes);
        if tenths < Self::TENTHS_SHOWN_UNDER {
            return Self::label_with_a_tenth(tenths, unit);
        }
        format!("{} {unit}", Self::rounded(bytes, unit_bytes))
    }

    fn label_with_a_tenth(tenths: u128, unit: &str) -> String {
        let whole = tenths / Self::TENTHS_PER_UNIT;
        let tenth = tenths % Self::TENTHS_PER_UNIT;
        format!("{whole}.{tenth} {unit}")
    }

    fn unit(self) -> Option<(&'static str, u128)> {
        let bytes = u128::from(self.bytes);
        Self::UNITS
            .into_iter()
            .zip(1..)
            .map(|(unit, power)| (unit, Self::STEP.pow(power)))
            .take_while(|(_, unit_bytes)| bytes >= *unit_bytes)
            .last()
    }

    fn rounded(amount: u128, divisor: u128) -> u128 {
        (amount + divisor / 2) / divisor
    }
}

impl Sum for MemorySize {
    fn sum<I: Iterator<Item = Self>>(sizes: I) -> Self {
        let bytes = sizes.fold(0, |total: u64, size| total.saturating_add(size.bytes));
        Self::of_bytes(bytes)
    }
}

impl fmt::Display for MemorySize {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.pad(&self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SizeFixture;

    impl SizeFixture {
        const KIB: u64 = 1024;
        const MIB: u64 = 1024 * Self::KIB;
        const GIB: u64 = 1024 * Self::MIB;

        fn shown(bytes: u64) -> String {
            MemorySize::of_bytes(bytes).to_string()
        }
    }

    #[test]
    fn a_size_under_a_kibibyte_shows_bytes() {
        assert_eq!(SizeFixture::shown(512), "512 B");
    }

    #[test]
    fn a_size_under_ten_units_shows_a_tenth() {
        assert_eq!(
            SizeFixture::shown(4 * SizeFixture::MIB + SizeFixture::MIB / 2),
            "4.5 MiB"
        );
    }

    #[test]
    fn a_size_of_ten_units_or_more_shows_whole_units() {
        assert_eq!(SizeFixture::shown(512 * SizeFixture::MIB), "512 MiB");
    }

    #[test]
    fn a_size_rounds_to_the_nearest_tenth() {
        let just_under_one_point_three = SizeFixture::GIB + SizeFixture::GIB * 26 / 100;

        assert_eq!(SizeFixture::shown(just_under_one_point_three), "1.3 GiB");
    }

    #[test]
    fn a_whole_kibibyte_shows_one_kibibyte() {
        assert_eq!(SizeFixture::shown(SizeFixture::KIB), "1.0 KiB");
    }

    #[test]
    fn sizes_add_up() {
        let total: MemorySize = [SizeFixture::MIB, 2 * SizeFixture::MIB]
            .map(MemorySize::of_bytes)
            .into_iter()
            .sum();

        assert_eq!(total, MemorySize::of_bytes(3 * SizeFixture::MIB));
    }

    #[test]
    fn a_size_pads_to_the_width_it_is_given() {
        assert_eq!(format!("{:>9}", MemorySize::of_bytes(512)), "    512 B");
    }
}
