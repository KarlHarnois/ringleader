#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeOutcome {
    UpToDate,
    Merged,
    Conflicted(Vec<String>),
}
