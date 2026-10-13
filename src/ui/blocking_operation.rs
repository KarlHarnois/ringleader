use crate::application::{
    CreateRequest, DeleteConfirmation, DeleteImpact, ServiceError, SessionService,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockingOperation {
    Restart(String),
    Create(CreateRequest),
    MeasureDelete(String),
    Delete { name: String, seen: DeleteImpact },
    Rename { old: String, new: String },
    Merge(String),
}

impl BlockingOperation {
    pub fn busy_message(&self) -> String {
        match self {
            Self::Restart(name) => format!("Restarting {name}…"),
            Self::Create(request) => format!("Creating {}…", request.name()),
            Self::MeasureDelete(name) => format!("Checking what deleting {name} would lose…"),
            Self::Delete { name, .. } => format!("Deleting {name}…"),
            Self::Rename { old, .. } => format!("Renaming {old}…"),
            Self::Merge(name) => format!("Fetching origin and merging it into {name}…"),
        }
    }

    pub fn run(self, service: &mut SessionService) -> OperationOutcome {
        match self {
            Self::Restart(name) => Self::restart(service, name),
            Self::Create(request) => match service.create_session(&request) {
                Ok(session) => OperationOutcome::done(
                    format!("Created {} on branch {}.", session.name(), session.branch()),
                    Some(session.name().to_string()),
                ),
                Err(error) => OperationOutcome::Failed(error),
            },
            Self::MeasureDelete(name) => Self::measure(service, name),
            Self::Delete { name, seen } => Self::delete(service, name, seen),
            Self::Rename { old, new } => Self::rename(service, &old, &new),
            Self::Merge(name) => Self::merge(service, name),
        }
    }

    fn merge(service: &mut SessionService, name: String) -> OperationOutcome {
        match service.merge_upstream(&name) {
            Ok(report) if report.needs_attention() => OperationOutcome::Warned {
                message: report.summary(),
                select: Some(name),
            },
            Ok(report) => OperationOutcome::done(report.summary(), Some(name)),
            Err(error) => OperationOutcome::Failed(error),
        }
    }

    fn restart(service: &mut SessionService, name: String) -> OperationOutcome {
        match service.restart_session(&name) {
            Ok(()) => OperationOutcome::done(format!("Restarted {name}."), Some(name)),
            Err(error) => OperationOutcome::Failed(error),
        }
    }

    fn rename(service: &mut SessionService, old: &str, new: &str) -> OperationOutcome {
        match service.rename_session(old, new) {
            Ok(session) => OperationOutcome::done(
                format!("Renamed {old} to {}.", session.name()),
                Some(session.name().to_string()),
            ),
            Err(error) => OperationOutcome::Failed(error),
        }
    }

    fn measure(service: &mut SessionService, name: String) -> OperationOutcome {
        match service.delete_impact(&name) {
            Ok(impact) => OperationOutcome::ConfirmDelete {
                name,
                impact,
                changed: false,
            },
            Err(error @ ServiceError::Unmeasurable(_)) => OperationOutcome::Failed(error.adding_context(&format!(
                "ringleader delete {name} --forget needs this TUI to quit first, since the TUI holds the lock"
            ))),
            Err(error) => OperationOutcome::Failed(error),
        }
    }

    fn delete(service: &mut SessionService, name: String, seen: DeleteImpact) -> OperationOutcome {
        match service.delete_session(&name, &DeleteConfirmation::confirmed(seen)) {
            Ok(outcome) => OperationOutcome::done(outcome.summary(&name), None),
            Err(ServiceError::GuardsChanged { impact, .. }) => OperationOutcome::ConfirmDelete {
                name,
                impact: *impact,
                changed: true,
            },
            Err(error) => OperationOutcome::Failed(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationOutcome {
    Done {
        message: String,
        select: Option<String>,
    },
    Warned {
        message: String,
        select: Option<String>,
    },
    ConfirmDelete {
        name: String,
        impact: DeleteImpact,
        changed: bool,
    },
    Failed(ServiceError),
}

impl OperationOutcome {
    fn done(message: String, select: Option<String>) -> Self {
        Self::Done { message, select }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::{MergeOutcome, MultiplexerError};
    use crate::test_support::{FakeMultiplexer, ServiceFixture};

    struct MergeOperationFixture;

    impl MergeOperationFixture {
        const NAME: &'static str = "fix-auth";

        fn conflicted(multiplexer: FakeMultiplexer) -> ServiceFixture {
            let session = ServiceFixture::session(Self::NAME);
            let conflicts = MergeOutcome::Conflicted(vec!["src/auth.rs".to_string()]);
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&session))
                .with_workspace(
                    ServiceFixture::workspace_for(&[&session]).with_upstream_merge(conflicts),
                )
                .with_host(ServiceFixture::host_for(&[&session]))
                .with_multiplexer(multiplexer.with_live(ServiceFixture::waiting(&session)))
        }

        fn agent_away_from_its_prompt() -> FakeMultiplexer {
            let not_ready = MultiplexerError::AgentNotReady {
                tmux_name: "ringleader-fix-auth".to_string(),
            };
            FakeMultiplexer::new().failing("type_if_ready", not_ready)
        }

        fn run(fixture: &ServiceFixture) -> OperationOutcome {
            BlockingOperation::Merge(Self::NAME.to_string()).run(&mut fixture.service())
        }
    }

    #[test]
    fn a_merge_announces_the_fetch_while_it_runs() {
        let operation = BlockingOperation::Merge(MergeOperationFixture::NAME.to_string());

        assert_eq!(
            operation.busy_message(),
            "Fetching origin and merging it into fix-auth…"
        );
    }

    #[test]
    fn conflicts_handed_to_the_agent_are_done() {
        let fixture = MergeOperationFixture::conflicted(FakeMultiplexer::new());

        let outcome = MergeOperationFixture::run(&fixture);

        assert_eq!(
            outcome,
            OperationOutcome::Done {
                message: "merging origin/main into fix-auth stopped on a conflict in 1 file, asked its agent to finish the merge".to_string(),
                select: Some("fix-auth".to_string()),
            }
        );
    }

    #[test]
    fn conflicts_the_agent_was_not_told_about_are_a_warning() {
        let fixture =
            MergeOperationFixture::conflicted(MergeOperationFixture::agent_away_from_its_prompt());

        let outcome = MergeOperationFixture::run(&fixture);

        assert!(
            matches!(outcome, OperationOutcome::Warned { .. }),
            "{outcome:?}"
        );
    }
}
