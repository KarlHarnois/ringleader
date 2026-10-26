use super::service_error::ServiceError;
use super::session_count::SessionCount;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchReport<T> {
    outcomes: Vec<(String, Result<T, ServiceError>)>,
}

impl<T> BatchReport<T> {
    const SKIPPED: &'static str = "skipped";
    const FAILED: &'static str = "failed";

    pub fn new(outcomes: Vec<(String, Result<T, ServiceError>)>) -> Self {
        Self { outcomes }
    }

    pub fn outcomes(&self) -> impl Iterator<Item = (&str, Result<&T, &ServiceError>)> {
        self.outcomes
            .iter()
            .map(|(name, outcome)| (name.as_str(), outcome.as_ref()))
    }

    pub fn succeeded(&self) -> impl Iterator<Item = (&str, &T)> {
        self.outcomes()
            .filter_map(|(name, outcome)| outcome.ok().map(|done| (name, done)))
    }

    pub fn failures(&self) -> impl Iterator<Item = (&str, &ServiceError)> {
        self.outcomes()
            .filter_map(|(name, outcome)| outcome.err().map(|error| (name, error)))
    }

    pub fn is_empty(&self) -> bool {
        self.outcomes.is_empty()
    }

    pub fn has_failures(&self) -> bool {
        self.failures().next().is_some()
    }

    pub fn summary(&self, past_tense: &str) -> String {
        let sentences: Vec<String> = std::iter::once(self.headline(past_tense))
            .chain(self.failure_note())
            .collect();
        sentences.join(" ")
    }

    pub fn headline(&self, past_tense: &str) -> String {
        let succeeded = self.succeeded().count();
        let total = self.outcomes.len();
        if succeeded == total {
            format!("{past_tense} {}.", SessionCount::new(total))
        } else {
            format!("{past_tense} {succeeded} of {}.", SessionCount::new(total))
        }
    }

    pub fn failure_note(&self) -> Option<String> {
        let mut failures = self.failures();
        let (name, error) = failures.next()?;
        let (skipped, failed): (Vec<_>, Vec<_>) =
            failures.partition(|(_, error)| error.is_refusal());
        let first = format!("{name} {}: {error}.", Self::verdict(error));
        let counts = [
            Self::more(skipped.len(), Self::SKIPPED),
            Self::more(failed.len(), Self::FAILED),
        ];
        let sentences: Vec<String> = std::iter::once(first)
            .chain(counts.into_iter().flatten())
            .collect();
        Some(sentences.join(" "))
    }

    fn verdict(error: &ServiceError) -> &'static str {
        match error.is_refusal() {
            true => Self::SKIPPED,
            false => Self::FAILED,
        }
    }

    fn more(count: usize, verdict: &str) -> Option<String> {
        (count > 0).then(|| format!("{count} more {verdict}."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct BatchFixture;

    impl BatchFixture {
        fn done(name: &str) -> (String, Result<(), ServiceError>) {
            (name.to_string(), Ok(()))
        }

        fn refused(name: &str) -> (String, Result<(), ServiceError>) {
            let reason = format!("session {name} is working");
            (name.to_string(), Err(ServiceError::Conflict(reason)))
        }

        fn failed(name: &str) -> (String, Result<(), ServiceError>) {
            let reason = format!("tmux could not end {name}");
            (name.to_string(), Err(ServiceError::Failed(reason)))
        }
    }

    #[test]
    fn a_batch_without_trouble_counts_its_sessions() {
        let report = BatchReport::new(vec![
            BatchFixture::done("alpha"),
            BatchFixture::done("bravo"),
        ]);

        assert_eq!(report.summary("Stopped"), "Stopped 2 sessions.");
        assert!(!report.has_failures());
    }

    #[test]
    fn a_batch_of_one_counts_one_session() {
        let report = BatchReport::new(vec![BatchFixture::done("alpha")]);

        assert_eq!(report.summary("Stopped"), "Stopped 1 session.");
    }

    #[test]
    fn a_refused_session_reads_as_skipped_because_its_state_forbids_the_action() {
        let report = BatchReport::new(vec![
            BatchFixture::refused("alpha"),
            BatchFixture::done("bravo"),
        ]);

        assert_eq!(
            report.summary("Stopped"),
            "Stopped 1 of 2 sessions. alpha skipped: session alpha is working."
        );
        assert!(report.has_failures());
    }

    #[test]
    fn a_session_that_broke_reads_as_failed() {
        let report = BatchReport::new(vec![BatchFixture::failed("alpha")]);

        assert_eq!(
            report.summary("Stopped"),
            "Stopped 0 of 1 session. alpha failed: tmux could not end alpha."
        );
    }

    #[test]
    fn the_trouble_after_the_first_is_counted_by_kind() {
        let report = BatchReport::new(vec![
            BatchFixture::failed("alpha"),
            BatchFixture::refused("bravo"),
            BatchFixture::failed("charlie"),
            BatchFixture::failed("delta"),
        ]);

        assert_eq!(
            report.failure_note().unwrap(),
            "alpha failed: tmux could not end alpha. 1 more skipped. 2 more failed."
        );
    }

    #[test]
    fn an_empty_batch_has_no_failure_note() {
        let report = BatchReport::<()>::new(Vec::new());

        assert!(report.is_empty());
        assert_eq!(report.failure_note(), None);
    }
}
