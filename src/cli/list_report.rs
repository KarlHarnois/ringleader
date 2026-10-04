use std::collections::HashMap;

use serde::Serialize;

use crate::domain::{DiffStat, SessionSummary};

pub struct ListReport<'a> {
    rows: &'a [SessionSummary],
    diffs: &'a HashMap<String, DiffStat>,
}

impl<'a> ListReport<'a> {
    const NO_DIFF: &'static str = "—";

    pub fn new(rows: &'a [SessionSummary], diffs: &'a HashMap<String, DiffStat>) -> Self {
        Self { rows, diffs }
    }

    pub fn lines(&self) -> Vec<String> {
        self.rows.iter().map(|row| self.line(row)).collect()
    }

    pub fn json(&self) -> String {
        let entries: Vec<ListEntry> = self.rows.iter().map(|row| self.entry(row)).collect();
        serde_json::to_string_pretty(&entries).unwrap_or_default()
    }

    fn line(&self, row: &SessionSummary) -> String {
        let metadata = row.metadata();
        let branch = metadata.map_or("", |metadata| metadata.branch());
        let worktree = metadata.map_or(String::new(), |metadata| {
            metadata.worktree_path().display().to_string()
        });
        let diff = self
            .diff(row)
            .map_or(Self::NO_DIFF.to_string(), |diff| diff.to_string());
        format!(
            "{:<9} {:<20} {:<30} {:<16} {}",
            row.state().label(),
            row.name(),
            branch,
            diff,
            worktree
        )
        .trim_end()
        .to_string()
    }

    fn entry(&self, row: &SessionSummary) -> ListEntry {
        let metadata = row.metadata();
        ListEntry {
            name: row.name().to_string(),
            repo: metadata.map(|metadata| metadata.repo_name().to_string()),
            branch: metadata.map(|metadata| metadata.branch().to_string()),
            worktree: metadata.map(|metadata| metadata.worktree_path().display().to_string()),
            agent: metadata.map(|metadata| metadata.agent().to_string()),
            state: row.state().label(),
            diff: self.diff(row).map(DiffEntry::from_stat),
        }
    }

    fn diff(&self, row: &SessionSummary) -> Option<DiffStat> {
        row.session()?;
        self.diffs.get(row.name()).copied()
    }
}

#[derive(Serialize)]
struct ListEntry {
    name: String,
    repo: Option<String>,
    branch: Option<String>,
    worktree: Option<String>,
    agent: Option<String>,
    state: &'static str,
    diff: Option<DiffEntry>,
}

#[derive(Serialize)]
struct DiffEntry {
    files: u64,
    insertions: u64,
    deletions: u64,
    untracked: u64,
}

impl DiffEntry {
    fn from_stat(diff: DiffStat) -> Self {
        Self {
            files: diff.files(),
            insertions: diff.insertions(),
            deletions: diff.deletions(),
            untracked: diff.untracked(),
        }
    }
}
