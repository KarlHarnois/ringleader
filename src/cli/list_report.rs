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

    pub fn result(&self) -> impl Serialize + '_ {
        ListResult {
            sessions: self.rows.iter().map(|row| self.entry(row)).collect(),
        }
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
            agent: metadata.map(|metadata| metadata.agent().to_string()),
            branch: metadata.map(|metadata| metadata.branch().to_string()),
            diff: self.diff(row).map(DiffEntry::from_stat),
            name: row.name().to_string(),
            repo: metadata.map(|metadata| metadata.repo_name().to_string()),
            state: row.state().label(),
            worktree: metadata.map(|metadata| metadata.worktree_path().display().to_string()),
        }
    }

    fn diff(&self, row: &SessionSummary) -> Option<DiffStat> {
        row.session()?;
        self.diffs.get(row.name()).copied()
    }
}

#[derive(Serialize)]
struct ListResult {
    sessions: Vec<ListEntry>,
}

#[derive(Serialize)]
struct ListEntry {
    agent: Option<String>,
    branch: Option<String>,
    diff: Option<DiffEntry>,
    name: String,
    repo: Option<String>,
    state: &'static str,
    worktree: Option<String>,
}

#[derive(Serialize)]
struct DiffEntry {
    deletions: u64,
    files: u64,
    insertions: u64,
    untracked: u64,
}

impl DiffEntry {
    fn from_stat(diff: DiffStat) -> Self {
        Self {
            deletions: diff.deletions(),
            files: diff.files(),
            insertions: diff.insertions(),
            untracked: diff.untracked(),
        }
    }
}
