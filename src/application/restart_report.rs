use super::batch_report::BatchReport;
use super::service_error::ServiceError;
use crate::application::StartOutcome;

pub struct RestartReport {
    restarts: BatchReport<StartOutcome>,
}

impl From<BatchReport<StartOutcome>> for RestartReport {
    fn from(restarts: BatchReport<StartOutcome>) -> Self {
        Self { restarts }
    }
}

impl RestartReport {
    const NOTHING_TO_RESTART: &'static str = "No exited or stopped session to restart.";
    const RESTARTED: &'static str = "Restarted";

    #[cfg(test)]
    pub fn new(restarts: Vec<(String, Result<StartOutcome, ServiceError>)>) -> Self {
        Self {
            restarts: BatchReport::new(restarts),
        }
    }

    pub fn outcomes(&self) -> impl Iterator<Item = (&str, Result<&StartOutcome, &ServiceError>)> {
        self.restarts.outcomes()
    }

    pub fn restarted(&self) -> impl Iterator<Item = &StartOutcome> {
        self.restarts.succeeded().map(|(_, outcome)| outcome)
    }

    pub fn failures(&self) -> impl Iterator<Item = (&str, &ServiceError)> {
        self.restarts.failures()
    }

    pub fn is_empty(&self) -> bool {
        self.restarts.is_empty()
    }

    pub fn needs_attention(&self) -> bool {
        self.restarts.has_failures() || self.warning_note().is_some()
    }

    pub fn summary(&self) -> String {
        if self.is_empty() {
            return Self::NOTHING_TO_RESTART.to_string();
        }
        let notes = [
            self.recovery_note(),
            self.restarts.failure_note(),
            self.warning_note(),
        ];
        let sentences: Vec<String> = std::iter::once(self.restarts.headline(Self::RESTARTED))
            .chain(notes.into_iter().flatten())
            .collect();
        sentences.join(" ")
    }

    fn recovery_note(&self) -> Option<String> {
        self.restarted()
            .find_map(StartOutcome::recovery)
            .map(|note| format!("Also {note}."))
    }

    fn warning_note(&self) -> Option<String> {
        self.restarted()
            .find_map(StartOutcome::warning)
            .map(|warning| format!("But {warning}."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ServiceFixture;

    struct ReportFixture;

    impl ReportFixture {
        fn restarted(name: &str) -> (String, Result<StartOutcome, ServiceError>) {
            let outcome = StartOutcome::new(ServiceFixture::session(name), None);
            (name.to_string(), Ok(outcome))
        }

        fn warned(name: &str, warning: &str) -> (String, Result<StartOutcome, ServiceError>) {
            let outcome =
                StartOutcome::new(ServiceFixture::session(name), Some(warning.to_string()));
            (name.to_string(), Ok(outcome))
        }

        fn recovered(name: &str, note: &str) -> (String, Result<StartOutcome, ServiceError>) {
            let outcome = StartOutcome::new(ServiceFixture::session(name), None)
                .with_recovery(Some(note.to_string()));
            (name.to_string(), Ok(outcome))
        }

        fn failed(name: &str, reason: &str) -> (String, Result<StartOutcome, ServiceError>) {
            (
                name.to_string(),
                Err(ServiceError::Invalid(reason.to_string())),
            )
        }
    }

    #[test]
    fn an_empty_report_says_nothing_needed_a_restart() {
        let report = RestartReport::new(Vec::new());

        assert_eq!(report.summary(), "No exited or stopped session to restart.");
        assert!(!report.needs_attention());
    }

    #[test]
    fn a_report_of_one_restart_counts_one_session() {
        let report = RestartReport::new(vec![ReportFixture::restarted("alpha")]);

        assert_eq!(report.summary(), "Restarted 1 session.");
    }

    #[test]
    fn a_report_without_trouble_counts_the_sessions_and_needs_no_attention() {
        let report = RestartReport::new(vec![
            ReportFixture::restarted("alpha"),
            ReportFixture::restarted("bravo"),
        ]);

        assert_eq!(report.summary(), "Restarted 2 sessions.");
        assert!(!report.needs_attention());
    }

    #[test]
    fn a_failure_is_named_with_its_reason_and_needs_attention() {
        let report = RestartReport::new(vec![
            ReportFixture::failed("alpha", "worktree /w/alpha is missing"),
            ReportFixture::restarted("bravo"),
        ]);

        assert_eq!(
            report.summary(),
            "Restarted 1 of 2 sessions. alpha failed: worktree /w/alpha is missing."
        );
        assert!(report.needs_attention());
    }

    #[test]
    fn failures_after_the_first_are_counted() {
        let report = RestartReport::new(vec![
            ReportFixture::failed("alpha", "worktree /w/alpha is missing"),
            ReportFixture::failed("bravo", "worktree /w/bravo is missing"),
            ReportFixture::failed("charlie", "worktree /w/charlie is missing"),
        ]);

        assert_eq!(
            report.summary(),
            "Restarted 0 of 3 sessions. alpha failed: worktree /w/alpha is missing. 2 more failed."
        );
    }

    #[test]
    fn a_recovered_socket_is_noted() {
        let report = RestartReport::new(vec![ReportFixture::recovered(
            "alpha",
            "recreated the socket",
        )]);

        assert_eq!(
            report.summary(),
            "Restarted 1 session. Also recreated the socket."
        );
        assert!(!report.needs_attention());
    }

    #[test]
    fn an_envrc_warning_is_kept_and_needs_attention() {
        let report = RestartReport::new(vec![ReportFixture::warned(
            "alpha",
            "its .envrc is not allowed",
        )]);

        assert_eq!(
            report.summary(),
            "Restarted 1 session. But its .envrc is not allowed."
        );
        assert!(report.needs_attention());
    }
}
