use std::fmt;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiffStat {
    files: u64,
    insertions: u64,
    deletions: u64,
    untracked: u64,
}

impl DiffStat {
    pub fn from_changes(changes: impl IntoIterator<Item = FileChange>, untracked: u64) -> Self {
        let tracked = changes
            .into_iter()
            .fold(Self::default(), |total, change| total.adding(change));
        Self {
            untracked,
            ..tracked
        }
    }

    pub fn files(&self) -> u64 {
        self.files
    }

    pub fn insertions(&self) -> u64 {
        self.insertions
    }

    pub fn deletions(&self) -> u64 {
        self.deletions
    }

    pub fn untracked(&self) -> u64 {
        self.untracked
    }

    fn adding(self, change: FileChange) -> Self {
        let (insertions, deletions) = change.line_counts();
        Self {
            files: self.files + 1,
            insertions: self.insertions + insertions,
            deletions: self.deletions + deletions,
            untracked: self.untracked,
        }
    }
}

impl fmt::Display for DiffStat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "+{} -{} ?{}",
            self.insertions, self.deletions, self.untracked
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChange {
    Text { insertions: u64, deletions: u64 },
    Binary,
}

impl FileChange {
    fn line_counts(self) -> (u64, u64) {
        match self {
            FileChange::Text {
                insertions,
                deletions,
            } => (insertions, deletions),
            FileChange::Binary => (0, 0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diff_stat_shows_insertions_deletions_and_untracked_files() {
        let diff = DiffStat::from_changes(
            [FileChange::Text {
                insertions: 12,
                deletions: 4,
            }],
            2,
        );

        assert_eq!(diff.to_string(), "+12 -4 ?2");
    }

    #[test]
    fn accumulates_text_changes() {
        let changes = [
            FileChange::Text {
                insertions: 10,
                deletions: 2,
            },
            FileChange::Text {
                insertions: 3,
                deletions: 4,
            },
        ];

        let diff = DiffStat::from_changes(changes, 0);

        assert_eq!(
            (diff.files(), diff.insertions(), diff.deletions()),
            (2, 13, 6)
        );
    }

    #[test]
    fn a_binary_file_counts_as_one_file_with_no_lines() {
        let changes = [
            FileChange::Binary,
            FileChange::Text {
                insertions: 1,
                deletions: 1,
            },
        ];

        let diff = DiffStat::from_changes(changes, 0);

        assert_eq!(
            (diff.files(), diff.insertions(), diff.deletions()),
            (2, 1, 1)
        );
    }

    #[test]
    fn keeps_the_untracked_count() {
        let diff = DiffStat::from_changes([], 4);

        assert_eq!(diff.untracked(), 4);
    }
}
