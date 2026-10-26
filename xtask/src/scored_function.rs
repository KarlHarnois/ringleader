use crate::line_coverage::LineCoverage;
use crate::source_function::SourceFunction;

pub struct ScoredFunction {
    function: SourceFunction,
    coverage: LineCoverage,
    crap: f64,
}

impl ScoredFunction {
    pub fn new(function: SourceFunction, coverage: LineCoverage) -> Self {
        let complexity = function.complexity() as f64;
        let crap = complexity * complexity * coverage.uncovered_fraction().powi(3) + complexity;
        Self {
            function,
            coverage,
            crap,
        }
    }

    pub fn function(&self) -> &SourceFunction {
        &self.function
    }

    pub fn coverage(&self) -> LineCoverage {
        self.coverage
    }

    pub fn crap(&self) -> f64 {
        self.crap
    }
}

#[cfg(test)]
mod tests {
    use super::ScoredFunction;
    use crate::line_coverage::LineCoverage;
    use crate::source_function::{LineSpan, SourceFunction};

    struct Scoring;

    impl Scoring {
        fn crap(complexity: usize, covered_lines: &[bool]) -> f64 {
            let function = SourceFunction::new(
                "src/shapes.rs".to_string(),
                "area".to_string(),
                LineSpan::new(1, 4),
                complexity,
            );
            let coverage: LineCoverage = covered_lines.iter().copied().collect();
            ScoredFunction::new(function, coverage).crap()
        }
    }

    #[test]
    fn a_fully_covered_function_scores_its_complexity() {
        assert_eq!(Scoring::crap(5, &[true, true]), 5.0);
    }

    #[test]
    fn an_uncovered_function_scores_complexity_squared_plus_complexity() {
        assert_eq!(Scoring::crap(5, &[false, false]), 30.0);
    }

    #[test]
    fn half_coverage_scores_an_eighth_of_complexity_squared_plus_complexity() {
        assert_eq!(Scoring::crap(10, &[true, false]), 22.5);
    }
}
