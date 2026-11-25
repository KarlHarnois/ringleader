use serde::Serialize;

use super::failed_entry::FailedEntry;
use super::{Remark, Reply};
use crate::application::{RestartReport, ServiceError, StartOutcome};

pub struct RestartReply {
    report: RestartReport,
}

impl RestartReply {
    const NOTHING_TO_RESTART: &'static str = "no exited or stopped session to restart";
    const RESTARTED: &'static str = "restarted";

    pub fn new(report: RestartReport) -> Self {
        Self { report }
    }

    fn restarted_lines(&self) -> Vec<String> {
        self.report
            .outcomes()
            .filter_map(|(name, outcome)| Self::restarted_line(name, outcome))
            .collect()
    }

    fn restarted_line(name: &str, outcome: Result<&StartOutcome, &ServiceError>) -> Option<String> {
        outcome.ok().map(|_| format!("{} {name}", Self::RESTARTED))
    }

    fn outcome_remarks(outcome: &StartOutcome) -> Vec<Remark> {
        [outcome.recovery(), outcome.warning()]
            .into_iter()
            .flatten()
            .map(|note| Remark::Note(note.to_string()))
            .collect()
    }

    fn entries(&self) -> Vec<RestartEntry> {
        self.report
            .outcomes()
            .map(|(name, outcome)| RestartEntry::of(name, outcome))
            .collect()
    }
}

impl Reply for RestartReply {
    fn lines(&self) -> Vec<String> {
        self.restarted_lines()
    }

    fn remarks(&self) -> Vec<Remark> {
        let mut remarks = Vec::new();
        if self.report.is_empty() {
            remarks.push(Remark::Note(Self::NOTHING_TO_RESTART.to_string()));
        }
        for (name, outcome) in self.report.outcomes() {
            match outcome {
                Ok(outcome) => remarks.extend(Self::outcome_remarks(outcome)),
                Err(error) => remarks.push(FailedEntry::of(name, error).remark()),
            }
        }
        remarks
    }

    fn result(&self) -> impl Serialize {
        RestartResult {
            results: self.entries(),
        }
    }
}

#[derive(Serialize)]
struct RestartResult {
    results: Vec<RestartEntry>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum RestartEntry {
    Restarted {
        name: String,
        result: &'static str,
        recovery: Option<String>,
        warning: Option<String>,
    },
    Failed(FailedEntry),
}

impl RestartEntry {
    fn of(name: &str, outcome: Result<&StartOutcome, &ServiceError>) -> Self {
        match outcome {
            Ok(outcome) => Self::Restarted {
                name: name.to_string(),
                result: RestartReply::RESTARTED,
                recovery: outcome.recovery().map(str::to_string),
                warning: outcome.warning().map(str::to_string),
            },
            Err(error) => Self::Failed(FailedEntry::of(name, error)),
        }
    }
}
