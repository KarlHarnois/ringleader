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
        let marker = if row.is_favorite() { "★" } else { " " };
        format!(
            "{:<9} {} {:<20} {:<30} {:<16} {}",
            row.state().label(),
            marker,
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
            favorite: row.is_favorite(),
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
    favorite: bool,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{SessionState, SessionSummary};
    use crate::test_support::ServiceFixture;

    #[test]
    fn a_favorite_marker_keeps_the_columns_aligned() {
        let session = ServiceFixture::session_at(
            "fix-the-reconciler",
            "/repos/alpha",
            ServiceFixture::CREATED_AT,
        )
        .with_favorite(true);
        let favorite = SessionSummary::managed(session, SessionState::Waiting, false);
        let plain = ServiceFixture::summary_in("docs", "/repos/alpha", SessionState::Waiting);
        let rows = [favorite, plain];

        let lines = ListReport::new(&rows, &HashMap::new()).lines();
        let column = |line: &str| {
            let byte = line.find("/worktrees").unwrap();
            line[..byte].chars().count()
        };

        assert_eq!(column(&lines[0]), column(&lines[1]));
    }
}
