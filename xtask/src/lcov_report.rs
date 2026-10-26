use std::collections::BTreeMap;
use std::path::Path;

use crate::line_coverage::LineCoverage;
use crate::source_function::LineSpan;

pub struct LcovReport {
    files: BTreeMap<String, BTreeMap<usize, bool>>,
}

impl LcovReport {
    const SOURCE_FILE: &str = "SF:";
    const LINE_HITS: &str = "DA:";

    pub fn parse(text: &str, root: &Path) -> Self {
        let mut report = Self {
            files: BTreeMap::new(),
        };
        let mut file = String::new();
        for record in text.lines() {
            if let Some(path) = record.strip_prefix(Self::SOURCE_FILE) {
                file = Self::relative(path, root);
            } else if let Some(line_hits) = record.strip_prefix(Self::LINE_HITS) {
                report.record(&file, line_hits);
            }
        }
        report
    }

    pub fn within(&self, file: &str, span: LineSpan) -> LineCoverage {
        self.files
            .get(file)
            .map(|lines| {
                lines
                    .range(span.first()..=span.last())
                    .map(|(_, covered)| *covered)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn record(&mut self, file: &str, line_hits: &str) {
        let mut fields = line_hits.split(',');
        let line = fields.next().and_then(|field| field.parse().ok());
        let hits = fields.next().and_then(|field| field.parse::<u64>().ok());
        if let (Some(line), Some(hits)) = (line, hits) {
            let covered = self
                .files
                .entry(file.to_string())
                .or_default()
                .entry(line)
                .or_default();
            *covered |= hits > 0;
        }
    }

    fn relative(path: &str, root: &Path) -> String {
        Path::new(path).strip_prefix(root).map_or_else(
            |_| path.to_string(),
            |relative| relative.display().to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::LcovReport;
    use crate::source_function::LineSpan;

    struct SampleReport;

    impl SampleReport {
        const TEXT: &str = "SF:/work/example-project/src/shapes.rs\n\
                            DA:3,1\nDA:4,0\nDA:5,0\nDA:6,2\nDA:9,0\nend_of_record\n\
                            SF:/work/example-project/src/main.rs\nDA:1,4\nend_of_record\n";

        fn coverage_of(file: &str, span: LineSpan) -> f64 {
            LcovReport::parse(Self::TEXT, Path::new("/work/example-project"))
                .within(file, span)
                .percent()
        }
    }

    #[test]
    fn within_counts_only_the_span() {
        assert_eq!(
            SampleReport::coverage_of("src/shapes.rs", LineSpan::new(5, 6)),
            50.0
        );
    }

    #[test]
    fn a_line_hit_by_any_record_counts_as_covered() {
        assert_eq!(
            SampleReport::coverage_of("src/main.rs", LineSpan::new(1, 1)),
            100.0
        );
    }

    #[test]
    fn an_unknown_file_has_no_coverage() {
        assert_eq!(
            SampleReport::coverage_of("src/other.rs", LineSpan::new(1, 9)),
            0.0
        );
    }
}
