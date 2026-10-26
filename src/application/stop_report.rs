use super::batch_report::BatchReport;
use super::service_error::ServiceError;
use crate::domain::{RepoGroup, StopSkip};

pub struct StopReport {
    repo: RepoGroup,
    stops: BatchReport<()>,
    skips: Vec<(String, StopSkip)>,
}

impl StopReport {
    const STOPPED: &'static str = "Stopped";

    pub fn new(repo: RepoGroup, stops: BatchReport<()>, skips: Vec<(String, StopSkip)>) -> Self {
        Self { repo, stops, skips }
    }

    pub fn outcomes(&self) -> impl Iterator<Item = (&str, Result<&(), &ServiceError>)> {
        self.stops.outcomes()
    }

    pub fn skips(&self) -> impl Iterator<Item = (&str, StopSkip)> {
        self.skips.iter().map(|(name, skip)| (name.as_str(), *skip))
    }

    pub fn failures(&self) -> impl Iterator<Item = (&str, &ServiceError)> {
        self.stops.failures()
    }

    pub fn needs_attention(&self) -> bool {
        self.stops.has_failures()
    }

    pub fn summary(&self) -> String {
        let sentences: Vec<String> = [
            Some(self.headline()),
            self.stops.failure_note(),
            self.skip_note(),
        ]
        .into_iter()
        .flatten()
        .collect();
        sentences.join(" ")
    }

    fn headline(&self) -> String {
        if self.stops.is_empty() {
            return format!("No waiting or exited session in {} to stop.", self.repo);
        }
        self.stops.headline(Self::STOPPED)
    }

    fn skip_note(&self) -> Option<String> {
        let counts = self.skip_counts();
        if counts.is_empty() {
            return None;
        }
        Some(format!("Skipped {}.", counts.join(", ")))
    }

    fn skip_counts(&self) -> Vec<String> {
        let mut counts: Vec<(StopSkip, usize)> = Vec::new();
        for (_, skip) in &self.skips {
            match counts.iter_mut().find(|(seen, _)| seen == skip) {
                Some((_, count)) => *count += 1,
                None => counts.push((*skip, 1)),
            }
        }
        counts
            .into_iter()
            .map(|(skip, count)| format!("{count} {}", skip.label()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::SessionState;
    use crate::test_support::ServiceFixture;

    struct ReportFixture;

    impl ReportFixture {
        fn report(
            stops: Vec<(String, Result<(), ServiceError>)>,
            skips: Vec<(String, StopSkip)>,
        ) -> StopReport {
            let repo = ServiceFixture::repo_group("/repos/alpha");
            StopReport::new(repo, BatchReport::new(stops), skips)
        }

        fn stopped(name: &str) -> (String, Result<(), ServiceError>) {
            (name.to_string(), Ok(()))
        }

        fn failed(name: &str, reason: &str) -> (String, Result<(), ServiceError>) {
            (
                name.to_string(),
                Err(ServiceError::Failed(reason.to_string())),
            )
        }

        fn working(name: &str) -> (String, StopSkip) {
            (name.to_string(), StopSkip::State(SessionState::Working))
        }

        fn main_checkout(name: &str) -> (String, StopSkip) {
            (name.to_string(), StopSkip::MainCheckout)
        }
    }

    #[test]
    fn a_report_of_every_stop_counts_the_sessions() {
        let report = ReportFixture::report(
            vec![
                ReportFixture::stopped("bravo"),
                ReportFixture::stopped("delta"),
            ],
            Vec::new(),
        );

        assert_eq!(report.summary(), "Stopped 2 sessions.");
        assert!(!report.needs_attention());
    }

    #[test]
    fn skipped_sessions_are_counted_by_reason() {
        let report = ReportFixture::report(
            vec![ReportFixture::stopped("bravo")],
            vec![
                ReportFixture::working("charlie"),
                ReportFixture::main_checkout("delta"),
                ReportFixture::working("echo"),
            ],
        );

        assert_eq!(
            report.summary(),
            "Stopped 1 session. Skipped 2 working, 1 in the main checkout."
        );
    }

    #[test]
    fn a_failed_stop_is_named_with_its_reason_and_needs_attention() {
        let report = ReportFixture::report(
            vec![
                ReportFixture::failed("bravo", "processes the agent started still run"),
                ReportFixture::stopped("delta"),
            ],
            Vec::new(),
        );

        assert_eq!(
            report.summary(),
            "Stopped 1 of 2 sessions. bravo failed: processes the agent started still run."
        );
        assert!(report.needs_attention());
    }

    #[test]
    fn a_repo_without_a_stoppable_session_names_the_repo_and_still_counts_the_skips() {
        let report = ReportFixture::report(Vec::new(), vec![ReportFixture::working("charlie")]);

        assert_eq!(
            report.summary(),
            "No waiting or exited session in alpha to stop. Skipped 1 working."
        );
    }
}
