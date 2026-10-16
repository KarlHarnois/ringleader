pub struct CompactCount {
    count: u64,
}

impl CompactCount {
    const THOUSAND: u64 = 1_000;
    const MILLION: u64 = 1_000_000;
    const TENTHS: u64 = 10;

    pub fn new(count: u64) -> Self {
        Self { count }
    }

    pub fn label(&self) -> String {
        if self.count < Self::THOUSAND {
            return self.count.to_string();
        }
        if self.count < Self::MILLION {
            return Self::tenths(self.count, Self::THOUSAND, "k");
        }
        Self::tenths(self.count, Self::MILLION, "M")
    }

    fn tenths(count: u64, unit: u64, suffix: &str) -> String {
        let tenths = count / (unit / Self::TENTHS);
        format!(
            "{}.{}{suffix}",
            tenths / Self::TENTHS,
            tenths % Self::TENTHS
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_count_under_a_thousand_shows_in_full() {
        assert_eq!(CompactCount::new(999).label(), "999");
    }

    #[test]
    fn a_count_in_the_thousands_shows_tenths_of_a_thousand() {
        assert_eq!(CompactCount::new(1_000).label(), "1.0k");
        assert_eq!(CompactCount::new(999_999).label(), "999.9k");
    }

    #[test]
    fn a_count_in_the_millions_shows_tenths_of_a_million() {
        assert_eq!(CompactCount::new(1_000_000).label(), "1.0M");
        assert_eq!(CompactCount::new(6_588_481).label(), "6.5M");
    }
}
