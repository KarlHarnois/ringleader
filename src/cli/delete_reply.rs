use std::path::Path;

use serde::Serialize;

use super::failed_entry::FailedEntry;
use super::impact_json::ImpactJson;
use super::{Remark, Reply};
use crate::application::{BatchReport, DeleteImpact, DeleteOutcome, ServiceError};

pub struct DeleteReply {
    report: BatchReport<DeleteOutcome>,
}

impl DeleteReply {
    const DELETED: &'static str = "deleted";
    const FORCE_HINT: &'static str = "pass --force to delete anyway";

    pub fn new(report: BatchReport<DeleteOutcome>) -> Self {
        Self { report }
    }

    fn deleted_lines(&self) -> Vec<String> {
        self.report
            .outcomes()
            .filter_map(|(name, outcome)| outcome.ok().map(|outcome| outcome.summary(name)))
            .collect()
    }

    fn outcome_remarks(&self) -> Vec<Remark> {
        let mut remarks = Vec::new();
        for (name, outcome) in self.report.outcomes() {
            match outcome {
                Ok(outcome) => remarks.extend(Self::warning_remarks(outcome)),
                Err(ServiceError::GuardsChanged { impact, .. }) => {
                    remarks.extend(Self::refusal_remarks(name, impact));
                }
                Err(error) => remarks.push(FailedEntry::of(name, error).remark()),
            }
        }
        remarks
    }

    fn warning_remarks(outcome: &DeleteOutcome) -> Vec<Remark> {
        outcome
            .losses()
            .into_iter()
            .chain(outcome.stray_session().map(str::to_string))
            .map(|warning| Remark::Note(format!("warning: {warning}")))
            .collect()
    }

    fn refusal_remarks(name: &str, impact: &DeleteImpact) -> Vec<Remark> {
        let mut lines = vec![format!("deleting {name} would lose work:")];
        lines.extend(
            impact
                .warnings()
                .iter()
                .map(|warning| format!("  - {warning}")),
        );
        lines.push(Self::FORCE_HINT.to_string());
        lines.into_iter().map(Remark::Note).collect()
    }

    fn entries(&self) -> Vec<DeleteEntry> {
        self.report
            .outcomes()
            .map(|(name, outcome)| DeleteEntry::of(name, outcome))
            .collect()
    }
}

impl Reply for DeleteReply {
    fn lines(&self) -> Vec<String> {
        self.deleted_lines()
    }

    fn remarks(&self) -> Vec<Remark> {
        self.outcome_remarks()
    }

    fn result(&self) -> impl Serialize {
        DeleteResult {
            results: self.entries(),
        }
    }
}

#[derive(Serialize)]
struct DeleteResult {
    results: Vec<DeleteEntry>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum DeleteEntry {
    Deleted {
        name: String,
        result: &'static str,
        impact: ImpactJson,
        kept: Option<KeptJson>,
        warning: Option<String>,
        summary: String,
    },
    Failed(FailedEntry),
}

impl DeleteEntry {
    fn of(name: &str, outcome: Result<&DeleteOutcome, &ServiceError>) -> Self {
        match outcome {
            Ok(outcome) => Self::Deleted {
                name: name.to_string(),
                result: DeleteReply::DELETED,
                impact: ImpactJson::with_warnings(outcome.impact(), outcome.losses()),
                kept: KeptJson::of(outcome.kept()),
                warning: outcome.stray_session().map(str::to_string),
                summary: outcome.summary(name),
            },
            Err(error) => Self::Failed(FailedEntry::of(name, error)),
        }
    }
}

#[derive(Serialize)]
struct KeptJson {
    path: String,
    branch: String,
}

impl KeptJson {
    fn of(kept: Option<(&Path, &str)>) -> Option<Self> {
        kept.map(|(path, branch)| Self {
            path: path.display().to_string(),
            branch: branch.to_string(),
        })
    }
}
