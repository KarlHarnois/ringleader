use serde::Serialize;

use super::failed_entry::FailedEntry;
use super::{Remark, Reply};
use crate::application::{BatchReport, ServiceError, StopReport};
use crate::domain::StopSkip;

pub struct StopReply {
    outcome: StopOutcome,
    nothing_to_stop: bool,
}

impl StopReply {
    const NOTHING_TO_STOP: &'static str = "no waiting or exited session to stop";
    const STOPPED: &'static str = "stopped";

    pub fn of_names(outcomes: BatchReport<()>) -> Self {
        Self {
            outcome: StopOutcome::Named(outcomes),
            nothing_to_stop: false,
        }
    }

    pub fn of_repo(report: StopReport) -> Self {
        let nothing_to_stop = report.outcomes().next().is_none();
        Self {
            outcome: StopOutcome::Repo(report),
            nothing_to_stop,
        }
    }

    fn outcomes(&self) -> Box<dyn Iterator<Item = (&str, Result<&(), &ServiceError>)> + '_> {
        match &self.outcome {
            StopOutcome::Named(outcomes) => Box::new(outcomes.outcomes()),
            StopOutcome::Repo(report) => Box::new(report.outcomes()),
        }
    }

    fn skips(&self) -> Box<dyn Iterator<Item = (&str, StopSkip)> + '_> {
        match &self.outcome {
            StopOutcome::Named(_) => Box::new(std::iter::empty()),
            StopOutcome::Repo(report) => Box::new(report.skips()),
        }
    }

    fn stopped_lines(&self) -> Vec<String> {
        self.outcomes()
            .filter_map(|(name, outcome)| Self::stopped_line(name, outcome))
            .collect()
    }

    fn stopped_line(name: &str, outcome: Result<&(), &ServiceError>) -> Option<String> {
        outcome.ok().map(|()| format!("{} {name}", Self::STOPPED))
    }

    fn skip_remarks(&self) -> Vec<Remark> {
        self.skips()
            .map(|(name, skip)| Remark::Note(format!("skipped {name}, {}", skip.label())))
            .collect()
    }

    fn problem_remarks(&self) -> Vec<Remark> {
        self.outcomes()
            .filter_map(|(name, outcome)| match outcome {
                Ok(()) => None,
                Err(error) => Some(FailedEntry::of(name, error).remark()),
            })
            .collect()
    }

    fn entries(&self) -> Vec<StopEntry> {
        self.outcomes()
            .map(|(name, outcome)| StopEntry::of(name, outcome))
            .collect()
    }

    fn skip_entries(&self) -> Vec<SkipEntry> {
        self.skips()
            .map(|(name, skip)| SkipEntry {
                name: name.to_string(),
                reason: skip.label().to_string(),
            })
            .collect()
    }
}

impl Reply for StopReply {
    fn lines(&self) -> Vec<String> {
        self.stopped_lines()
    }

    fn remarks(&self) -> Vec<Remark> {
        let mut remarks = self.skip_remarks();
        if self.nothing_to_stop {
            remarks.push(Remark::Note(Self::NOTHING_TO_STOP.to_string()));
        }
        remarks.extend(self.problem_remarks());
        remarks
    }

    fn result(&self) -> impl Serialize {
        StopResult {
            results: self.entries(),
            skipped: self.skip_entries(),
        }
    }
}

enum StopOutcome {
    Named(BatchReport<()>),
    Repo(StopReport),
}

#[derive(Serialize)]
struct StopResult {
    results: Vec<StopEntry>,
    skipped: Vec<SkipEntry>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum StopEntry {
    Stopped { name: String, result: &'static str },
    Failed(FailedEntry),
}

impl StopEntry {
    fn of(name: &str, outcome: Result<&(), &ServiceError>) -> Self {
        match outcome {
            Ok(()) => Self::Stopped {
                name: name.to_string(),
                result: StopReply::STOPPED,
            },
            Err(error) => Self::Failed(FailedEntry::of(name, error)),
        }
    }
}

#[derive(Serialize)]
struct SkipEntry {
    name: String,
    reason: String,
}
