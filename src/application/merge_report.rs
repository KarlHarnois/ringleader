use super::ports::{MergeOutcome, MultiplexerError};
use crate::domain::{BaseBranch, Branch, SessionName};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeReport {
    name: SessionName,
    merge: UpstreamMerge,
    handoff: ConflictHandoff,
}

impl MergeReport {
    pub(super) fn new(name: &SessionName, merge: UpstreamMerge, handoff: ConflictHandoff) -> Self {
        Self {
            name: name.clone(),
            merge,
            handoff,
        }
    }

    pub fn needs_attention(&self) -> bool {
        matches!(self.handoff, ConflictHandoff::Refused(_))
    }

    pub fn summary(&self) -> String {
        let name = &self.name;
        let upstream = self.merge.upstream();
        match (self.merge.outcome(), &self.handoff) {
            (MergeOutcome::UpToDate, _) => format!("{name} already has {upstream}"),
            (MergeOutcome::Merged, _) => format!("merged {upstream} into {name}"),
            (MergeOutcome::Conflicted(paths), ConflictHandoff::Refused(error)) => format!(
                "merging {upstream} into {name} stopped on {}, and its agent was not asked to finish the merge: {error}",
                Self::conflicts(paths)
            ),
            (MergeOutcome::Conflicted(paths), _) => format!(
                "merging {upstream} into {name} stopped on {}, asked its agent to finish the merge",
                Self::conflicts(paths)
            ),
        }
    }

    fn conflicts(paths: &[String]) -> String {
        match paths.len() {
            1 => "a conflict in 1 file".to_string(),
            count => format!("conflicts in {count} files"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UpstreamMerge {
    upstream: Branch,
    outcome: MergeOutcome,
}

impl UpstreamMerge {
    pub(super) fn new(branch_on_origin: &BaseBranch, outcome: MergeOutcome) -> Self {
        Self {
            upstream: branch_on_origin.origin_copy(),
            outcome,
        }
    }

    pub(super) fn upstream(&self) -> &str {
        self.upstream.name()
    }

    pub(super) fn outcome(&self) -> &MergeOutcome {
        &self.outcome
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ConflictHandoff {
    Unneeded,
    Asked,
    Refused(MultiplexerError),
}
