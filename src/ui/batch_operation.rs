use super::blocking_operation::OperationOutcome;
use crate::application::{
    DeleteConfirmation, DeleteImpact, RestartReport, SessionCount, SessionService,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchOperation {
    RestartAll,
    Restart(Vec<String>),
    Stop(Vec<String>),
    Merge(Vec<String>),
    Delete(DeleteSessionsStep),
}

impl BatchOperation {
    const STOPPED: &'static str = "Stopped";
    const MERGED: &'static str = "Merged origin into";

    pub fn changes_state(&self) -> bool {
        !matches!(self, Self::Delete(DeleteSessionsStep::Measure(_)))
    }

    pub fn busy_message(&self) -> String {
        match self {
            Self::RestartAll => "Restarting every exited or stopped session…".to_string(),
            Self::Restart(names) => format!("Restarting {}…", SessionCount::new(names.len())),
            Self::Stop(names) => format!("Stopping {}…", SessionCount::new(names.len())),
            Self::Merge(names) => format!(
                "Fetching origin and merging it into {}…",
                SessionCount::new(names.len())
            ),
            Self::Delete(step) => step.busy_message(),
        }
    }

    pub fn run(self, service: &mut SessionService) -> OperationOutcome {
        match self {
            Self::RestartAll => Self::restart_all(service),
            Self::Restart(names) => Self::restarted(&service.restart_sessions(&names, || false)),
            Self::Stop(names) => {
                OperationOutcome::reported(&service.stop_sessions(&names, || false), Self::STOPPED)
            }
            Self::Merge(names) => Self::merge(service, &names),
            Self::Delete(step) => step.run(service),
        }
    }

    fn restart_all(service: &mut SessionService) -> OperationOutcome {
        match service.restart_all(|| false) {
            Ok(report) => Self::restarted(&report),
            Err(error) => OperationOutcome::Failed(error),
        }
    }

    fn restarted(report: &RestartReport) -> OperationOutcome {
        OperationOutcome::batch(report.summary(), report.needs_attention())
    }

    fn merge(service: &mut SessionService, names: &[String]) -> OperationOutcome {
        let report = service.merge_upstream_sessions(names);
        let conflicts = report
            .succeeded()
            .filter(|(_, merge)| merge.needs_attention())
            .map(|(_, merge)| format!("But {}.", merge.summary()));
        let sentences: Vec<String> = std::iter::once(report.summary(Self::MERGED))
            .chain(conflicts)
            .collect();
        let needs_attention = report.has_failures() || sentences.len() > 1;
        OperationOutcome::batch(sentences.join(" "), needs_attention)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteSessionsStep {
    Measure(Vec<String>),
    Confirmed(Vec<(String, DeleteImpact)>),
}

impl DeleteSessionsStep {
    const DELETED: &'static str = "Deleted";

    fn busy_message(&self) -> String {
        match self {
            Self::Measure(names) => format!(
                "Checking what deleting {} would lose…",
                SessionCount::new(names.len())
            ),
            Self::Confirmed(seen) => format!("Deleting {}…", SessionCount::new(seen.len())),
        }
    }

    fn run(self, service: &mut SessionService) -> OperationOutcome {
        match self {
            Self::Measure(names) => {
                OperationOutcome::ConfirmDeleteSessions(service.delete_impacts(&names))
            }
            Self::Confirmed(seen) => Self::delete(service, seen),
        }
    }

    fn delete(service: &mut SessionService, seen: Vec<(String, DeleteImpact)>) -> OperationOutcome {
        let confirmations: Vec<(String, DeleteConfirmation)> = seen
            .into_iter()
            .map(|(name, impact)| (name, DeleteConfirmation::confirmed(impact)))
            .collect();
        let report = service.delete_sessions(&confirmations, || false);
        OperationOutcome::reported(&report, Self::DELETED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::MergeOutcome;
    use crate::test_support::{FakeMultiplexer, ServiceFixture, UiHarness};

    struct BatchOperationFixture;

    impl BatchOperationFixture {
        const NAME: &'static str = "fix-auth";

        fn names() -> Vec<String> {
            vec![Self::NAME.to_string()]
        }

        fn deletable() -> ServiceFixture {
            let session = ServiceFixture::session(Self::NAME);
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&session))
                .with_workspace(ServiceFixture::workspace_for(&[&session]))
                .with_host(ServiceFixture::host_for(&[&session]))
        }

        fn merging(outcome: MergeOutcome, multiplexer: FakeMultiplexer) -> ServiceFixture {
            ServiceFixture::merging(Self::NAME, outcome, multiplexer)
        }

        fn conflicts() -> MergeOutcome {
            MergeOutcome::Conflicted(vec!["src/auth.rs".to_string()])
        }

        fn done(message: &str) -> OperationOutcome {
            OperationOutcome::done(message.to_string(), None)
        }

        fn is_warning(outcome: &OperationOutcome) -> bool {
            matches!(outcome, OperationOutcome::Warned { .. })
        }
    }

    #[test]
    fn each_batch_counts_its_sessions_while_it_runs() {
        let names = ["alpha", "bravo"].map(String::from).to_vec();
        let seen = names
            .iter()
            .map(|name| (name.clone(), DeleteImpact::default()))
            .collect();

        let messages = [
            BatchOperation::Restart(names.clone()),
            BatchOperation::Stop(names.clone()),
            BatchOperation::Merge(names.clone()),
            BatchOperation::Delete(DeleteSessionsStep::Measure(names)),
            BatchOperation::Delete(DeleteSessionsStep::Confirmed(seen)),
        ]
        .map(|operation| operation.busy_message());

        assert_eq!(
            messages,
            [
                "Restarting 2 sessions…",
                "Stopping 2 sessions…",
                "Fetching origin and merging it into 2 sessions…",
                "Checking what deleting 2 sessions would lose…",
                "Deleting 2 sessions…",
            ]
        );
    }

    #[test]
    fn restart_all_announces_every_exited_or_stopped_session() {
        assert_eq!(
            BatchOperation::RestartAll.busy_message(),
            "Restarting every exited or stopped session…"
        );
    }

    #[test]
    fn only_measuring_a_batch_delete_leaves_the_state_alone() {
        let measure = BatchOperation::Delete(DeleteSessionsStep::Measure(Vec::new()));
        let stop = BatchOperation::Stop(Vec::new());

        assert!(!measure.changes_state());
        assert!(stop.changes_state());
    }

    #[test]
    fn a_batch_restart_counts_the_restarted_sessions() {
        let fixture = UiHarness::exited_session_fixture(BatchOperationFixture::NAME);
        let operation = BatchOperation::Restart(BatchOperationFixture::names());

        let outcome = operation.run(&mut fixture.service());

        assert_eq!(outcome, BatchOperationFixture::done("Restarted 1 session."));
    }

    #[test]
    fn restart_all_counts_the_restarted_sessions() {
        let fixture = UiHarness::exited_session_fixture(BatchOperationFixture::NAME);

        let outcome = BatchOperation::RestartAll.run(&mut fixture.service());

        assert_eq!(outcome, BatchOperationFixture::done("Restarted 1 session."));
    }

    #[test]
    fn a_batch_stop_counts_the_stopped_sessions() {
        let fixture = UiHarness::exited_session_fixture(BatchOperationFixture::NAME);
        let operation = BatchOperation::Stop(BatchOperationFixture::names());

        let outcome = operation.run(&mut fixture.service());

        assert_eq!(outcome, BatchOperationFixture::done("Stopped 1 session."));
    }

    #[test]
    fn a_skipped_session_turns_the_batch_into_a_warning() {
        let fixture = UiHarness::exited_session_fixture(BatchOperationFixture::NAME);
        let operation = BatchOperation::Stop(vec!["missing".to_string()]);

        let outcome = operation.run(&mut fixture.service());

        assert!(BatchOperationFixture::is_warning(&outcome), "{outcome:?}");
    }

    #[test]
    fn a_clean_batch_merge_counts_the_merged_sessions() {
        let fixture = BatchOperationFixture::merging(MergeOutcome::Merged, FakeMultiplexer::new());
        let operation = BatchOperation::Merge(BatchOperationFixture::names());

        let outcome = operation.run(&mut fixture.service());

        assert_eq!(
            outcome,
            BatchOperationFixture::done("Merged origin into 1 session.")
        );
    }

    #[test]
    fn conflicts_the_agent_was_not_told_about_turn_the_batch_merge_into_a_warning() {
        let multiplexer = ServiceFixture::refusing_typing(BatchOperationFixture::NAME);
        let fixture =
            BatchOperationFixture::merging(BatchOperationFixture::conflicts(), multiplexer);
        let operation = BatchOperation::Merge(BatchOperationFixture::names());

        let outcome = operation.run(&mut fixture.service());

        let OperationOutcome::Warned { message, .. } = outcome else {
            panic!("expected a warning, got {outcome:?}");
        };
        assert!(
            message.contains("But merging origin/main into fix-auth stopped"),
            "{message}"
        );
    }

    #[test]
    fn measuring_a_batch_delete_asks_for_one_confirmation() {
        let fixture = BatchOperationFixture::deletable();
        let operation =
            BatchOperation::Delete(DeleteSessionsStep::Measure(BatchOperationFixture::names()));

        let outcome = operation.run(&mut fixture.service());

        assert!(
            matches!(outcome, OperationOutcome::ConfirmDeleteSessions(_)),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_confirmed_batch_delete_counts_the_deleted_sessions() {
        let fixture = BatchOperationFixture::deletable();
        let seen = vec![(
            BatchOperationFixture::NAME.to_string(),
            DeleteImpact::default(),
        )];
        let operation = BatchOperation::Delete(DeleteSessionsStep::Confirmed(seen));

        let outcome = operation.run(&mut fixture.service());

        assert_eq!(outcome, BatchOperationFixture::done("Deleted 1 session."));
        assert!(fixture.registry().sessions().is_empty());
    }
}
