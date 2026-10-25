use super::service_error::ServiceError;

pub struct NamedOutcomes<T> {
    outcomes: Vec<(String, Result<T, ServiceError>)>,
}

impl<T> NamedOutcomes<T> {
    pub fn new(outcomes: Vec<(String, Result<T, ServiceError>)>) -> Self {
        Self { outcomes }
    }

    pub fn outcomes(&self) -> impl Iterator<Item = (&str, Result<&T, &ServiceError>)> {
        self.outcomes
            .iter()
            .map(|(name, outcome)| (name.as_str(), outcome.as_ref()))
    }

    pub fn succeeded(&self) -> impl Iterator<Item = &T> {
        self.outcomes
            .iter()
            .filter_map(|(_, outcome)| outcome.as_ref().ok())
    }

    pub fn failures(&self) -> impl Iterator<Item = (&str, &ServiceError)> {
        self.outcomes.iter().filter_map(|(name, outcome)| {
            outcome.as_ref().err().map(|error| (name.as_str(), error))
        })
    }

    pub fn is_empty(&self) -> bool {
        self.outcomes.is_empty()
    }

    pub fn tally(&self, verb: &str) -> String {
        let succeeded = self.succeeded().count();
        let total = self.outcomes.len();
        if succeeded == total {
            format!("{verb} {}", Self::sessions(total))
        } else {
            format!("{verb} {succeeded} of {}", Self::sessions(total))
        }
    }

    pub fn failure_note(&self) -> Option<String> {
        let mut failures = self.failures();
        let (name, error) = failures.next()?;
        let first = format!("{name} failed: {error}.");
        match failures.count() {
            0 => Some(first),
            more => Some(format!("{first} {more} more failed.")),
        }
    }

    fn sessions(count: usize) -> String {
        match count {
            1 => "1 session".to_string(),
            count => format!("{count} sessions"),
        }
    }
}
