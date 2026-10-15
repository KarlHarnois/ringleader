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

#[cfg(test)]
mod tests {
    use super::*;

    struct ReportFixture;

    impl ReportFixture {
        fn report(outcome: MergeOutcome, handoff: ConflictHandoff) -> MergeReport {
            let name = SessionName::new("fix-auth").unwrap();
            let main = BaseBranch::new("main").unwrap();
            MergeReport::new(&name, UpstreamMerge::new(&main, outcome), handoff)
        }

        fn conflicts(count: usize) -> MergeOutcome {
            let paths = (1..=count).map(|number| format!("src/file{number}.rs"));
            MergeOutcome::Conflicted(paths.collect())
        }

        fn not_at_its_prompt() -> ConflictHandoff {
            ConflictHandoff::Refused(MultiplexerError::AgentNotReady {
                tmux_name: "ringleader-fix-auth".to_string(),
            })
        }
    }

    #[test]
    fn an_up_to_date_session_is_summarized() {
        let report = ReportFixture::report(MergeOutcome::UpToDate, ConflictHandoff::Unneeded);

        assert_eq!(report.summary(), "fix-auth already has origin/main");
    }

    #[test]
    fn a_merge_is_summarized() {
        let report = ReportFixture::report(MergeOutcome::Merged, ConflictHandoff::Unneeded);

        assert_eq!(report.summary(), "merged origin/main into fix-auth");
    }

    #[test]
    fn conflicts_asked_of_the_agent_are_summarized() {
        let report = ReportFixture::report(ReportFixture::conflicts(2), ConflictHandoff::Asked);

        assert_eq!(
            report.summary(),
            "merging origin/main into fix-auth stopped on conflicts in 2 files, asked its agent to finish the merge"
        );
        assert!(!report.needs_attention());
    }

    #[test]
    fn a_single_conflict_is_named_in_the_singular() {
        let report = ReportFixture::report(ReportFixture::conflicts(1), ConflictHandoff::Asked);

        assert_eq!(
            report.summary(),
            "merging origin/main into fix-auth stopped on a conflict in 1 file, asked its agent to finish the merge"
        );
    }

    #[test]
    fn conflicts_the_agent_was_not_asked_about_need_attention_and_say_why() {
        let report = ReportFixture::report(
            ReportFixture::conflicts(2),
            ReportFixture::not_at_its_prompt(),
        );

        assert!(report.needs_attention());
        assert_eq!(
            report.summary(),
            "merging origin/main into fix-auth stopped on conflicts in 2 files, and its agent was not asked to finish the merge: the agent in tmux session ringleader-fix-auth is not at its prompt"
        );
    }
}
