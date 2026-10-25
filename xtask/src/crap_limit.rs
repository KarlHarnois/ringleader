use crate::scored_function::ScoredFunction;

pub struct CrapLimit {
    maximum: f64,
    functions: Vec<ScoredFunction>,
}

impl CrapLimit {
    pub fn new(maximum: f64, mut functions: Vec<ScoredFunction>) -> Self {
        functions.sort_by(|left, right| right.crap().total_cmp(&left.crap()));
        Self { maximum, functions }
    }

    pub fn passes(&self) -> bool {
        self.over_limit().next().is_none()
    }

    pub fn summary(&self) -> String {
        match self.functions.first() {
            Some(worst) => format!(
                "the worst of {} CRAP scores is {:.1}, in {}, and the limit is {}",
                self.functions.len(),
                worst.crap(),
                worst.function(),
                self.maximum
            ),
            None => "no function to score".to_string(),
        }
    }

    pub fn details(&self) -> String {
        let rows: String = self.over_limit().map(Self::row).collect();
        format!(
            "Split these functions or test their branches. A function that no test can reach \
             must have complexity 2 or less.\n     CRAP   CC  covered  function\n{rows}"
        )
    }

    fn over_limit(&self) -> impl Iterator<Item = &ScoredFunction> {
        self.functions
            .iter()
            .take_while(|scored| scored.crap() > self.maximum)
    }

    fn row(scored: &ScoredFunction) -> String {
        format!(
            "  {:>7.1} {:>4} {:>7.1}%  {}\n",
            scored.crap(),
            scored.function().complexity(),
            scored.coverage().percent(),
            scored.function()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::CrapLimit;
    use crate::line_coverage::LineCoverage;
    use crate::scored_function::ScoredFunction;
    use crate::source_function::{LineSpan, SourceFunction};

    struct Limit;

    impl Limit {
        const MAXIMUM: f64 = 8.0;

        fn over(complexities: &[(&str, usize)]) -> CrapLimit {
            let functions = complexities
                .iter()
                .map(|(name, complexity)| Self::covered(name, *complexity))
                .collect();
            CrapLimit::new(Self::MAXIMUM, functions)
        }

        fn covered(name: &str, complexity: usize) -> ScoredFunction {
            let function = SourceFunction::new(
                "src/main.rs".to_string(),
                name.to_string(),
                LineSpan::new(1, 4),
                complexity,
            );
            ScoredFunction::new(function, [true].into_iter().collect::<LineCoverage>())
        }
    }

    #[test]
    fn a_function_over_the_limit_fails_the_gate() {
        assert!(!Limit::over(&[("simple", 2), ("branchy", 9)]).passes());
    }

    #[test]
    fn the_summary_names_the_worst_function() {
        let summary = Limit::over(&[("simple", 2), ("branchy", 9)]).summary();
        assert!(summary.contains("9.0, in src/main.rs:1 branchy"));
    }

    #[test]
    fn the_details_list_only_the_functions_over_the_limit() {
        let details = Limit::over(&[("simple", 2), ("branchy", 9)]).details();
        assert!(details.contains("branchy"));
        assert!(!details.contains("simple"));
    }

    #[test]
    fn a_function_at_the_limit_passes() {
        assert!(Limit::over(&[("simple", 8)]).passes());
    }
}
