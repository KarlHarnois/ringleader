#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LineCoverage {
    covered: usize,
    total: usize,
}

impl LineCoverage {
    const PERCENT: f64 = 100.0;

    pub fn percent(self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        Self::PERCENT * self.covered as f64 / self.total as f64
    }

    pub fn uncovered_fraction(self) -> f64 {
        1.0 - self.percent() / Self::PERCENT
    }

    fn with_line(self, covered: bool) -> Self {
        Self {
            covered: self.covered + usize::from(covered),
            total: self.total + 1,
        }
    }
}

impl FromIterator<bool> for LineCoverage {
    fn from_iter<Lines: IntoIterator<Item = bool>>(lines: Lines) -> Self {
        lines
            .into_iter()
            .fold(Self::default(), |coverage, covered| {
                coverage.with_line(covered)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::LineCoverage;

    #[test]
    fn percent_counts_covered_lines() {
        let coverage: LineCoverage = [true, true, true, false].into_iter().collect();
        assert_eq!(coverage.percent(), 75.0);
        assert_eq!(coverage.uncovered_fraction(), 0.25);
    }

    #[test]
    fn no_lines_count_as_uncovered() {
        assert_eq!(LineCoverage::default().percent(), 0.0);
    }
}
