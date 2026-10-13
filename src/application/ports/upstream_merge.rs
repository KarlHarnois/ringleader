#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamMerge {
    upstream: String,
    outcome: MergeOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeOutcome {
    UpToDate,
    Merged,
    Conflicted(Vec<String>),
}

impl UpstreamMerge {
    pub fn new(upstream: &str, outcome: MergeOutcome) -> Self {
        Self {
            upstream: upstream.to_string(),
            outcome,
        }
    }

    pub fn upstream(&self) -> &str {
        &self.upstream
    }

    pub fn outcome(&self) -> &MergeOutcome {
        &self.outcome
    }
}
