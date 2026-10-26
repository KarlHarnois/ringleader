use super::batch_operation::BatchOperation;
use crate::application::{
    BatchReport, CreateRequest, DeleteConfirmation, DeleteImpact, DeleteOutcome, ServiceError,
    SessionService, StartOutcome,
};
use crate::domain::RepoGroup;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockingOperation {
    Restart(String),
    Stop(String),
    Create(CreateRequest),
    Delete(DeleteStep),
    Rename { old: String, new: String },
    Merge(String),
    Open(BrowserPage),
    Batch(BatchOperation),
}

impl BlockingOperation {
    pub fn changes_state(&self) -> bool {
        match self {
            Self::Delete(DeleteStep::Measure(_)) | Self::Open(_) => false,
            Self::Batch(batch) => batch.changes_state(),
            _ => true,
        }
    }

    pub fn reads_sessions(&self) -> bool {
        !matches!(self, Self::Open(BrowserPage::PullRequest(_)))
    }

    pub fn busy_message(&self) -> String {
        match self {
            Self::Restart(name) => format!("Restarting {name}…"),
            Self::Stop(name) => format!("Stopping {name}…"),
            Self::Create(request) => format!("Creating {}…", request.name()),
            Self::Delete(step) => step.busy_message(),
            Self::Rename { old, .. } => format!("Renaming {old}…"),
            Self::Merge(name) => format!("Fetching origin and merging it into {name}…"),
            Self::Open(page) => page.busy_message(),
            Self::Batch(batch) => batch.busy_message(),
        }
    }

    pub fn run(self, service: &mut SessionService) -> OperationOutcome {
        match self {
            Self::Restart(name) => Self::restart(service, name),
            Self::Stop(name) => Self::stop(service, name),
            Self::Create(request) => Self::create(service, &request),
            Self::Delete(step) => step.run(service),
            Self::Rename { old, new } => Self::rename(service, &old, &new),
            Self::Merge(name) => Self::merge(service, name),
            Self::Open(page) => page.run(service),
            Self::Batch(batch) => batch.run(service),
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

    fn create(service: &mut SessionService, request: &CreateRequest) -> OperationOutcome {
        match service.create_session(request) {
            Ok(outcome) => OperationOutcome::started(Self::created(&outcome), &outcome),
            Err(error) => OperationOutcome::Failed(error),
        }
    }

    fn created(outcome: &StartOutcome) -> String {
        let session = outcome.session();
        format!("Created {} on branch {}.", session.name(), session.branch())
    }

    fn restart(service: &mut SessionService, name: String) -> OperationOutcome {
        match service.restart_session(&name) {
            Ok(outcome) => {
                OperationOutcome::started(Self::restarted(&name, outcome.recovery()), &outcome)
            }
            Err(error) => OperationOutcome::Failed(error),
        }
    }

    fn restarted(name: &str, recovery: Option<&str>) -> String {
        match recovery {
            Some(note) => format!("Restarted {name}. Also {note}."),
            None => format!("Restarted {name}."),
        }
    }

    fn stop(service: &mut SessionService, name: String) -> OperationOutcome {
        match service.stop_session(&name) {
            Ok(()) => {
                OperationOutcome::done(format!("Stopped {name}. Press s to resume it."), Some(name))
            }
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserPage {
    RepoPage(RepoGroup),
    PullRequest(String),
}

impl BrowserPage {
    fn busy_message(&self) -> String {
        match self {
            Self::RepoPage(group) => format!("Looking up the GitHub page of {group}…"),
            Self::PullRequest(name) => format!("Finding the pull request of {name}…"),
        }
    }

    fn run(self, service: &SessionService) -> OperationOutcome {
        match self {
            Self::RepoPage(group) => Self::open_repo_page(service, &group),
            Self::PullRequest(name) => Self::open_pull_request(service, &name),
        }
    }

    fn open_repo_page(service: &SessionService, group: &RepoGroup) -> OperationOutcome {
        match service.open_repo_page(group) {
            Ok(()) => OperationOutcome::done(
                format!("Asked the browser to open the GitHub page of {group}."),
                None,
            ),
            Err(error) => OperationOutcome::Failed(error),
        }
    }

    fn open_pull_request(service: &SessionService, name: &str) -> OperationOutcome {
        match service.open_pull_request(name) {
            Ok(()) => OperationOutcome::Opened {
                name: name.to_string(),
                message: format!("Asked the browser to open the pull request of {name}."),
            },
            Err(error) => OperationOutcome::Failed(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteStep {
    Measure(String),
    Confirmed { name: String, seen: DeleteImpact },
}

impl DeleteStep {
    fn busy_message(&self) -> String {
        match self {
            Self::Measure(name) => format!("Checking what deleting {name} would lose…"),
            Self::Confirmed { name, .. } => format!("Deleting {name}…"),
        }
    }

    fn run(self, service: &mut SessionService) -> OperationOutcome {
        match self {
            Self::Measure(name) => Self::measure(service, name),
            Self::Confirmed { name, seen } => Self::delete(service, name, seen),
        }
    }

    fn measure(service: &mut SessionService, name: String) -> OperationOutcome {
        match service.delete_impact(&name) {
            Ok(impact) => OperationOutcome::ConfirmDelete {
                name,
                impact,
                changed: false,
            },
            Err(error @ ServiceError::Unmeasurable(_)) => {
                OperationOutcome::Failed(error.adding_context(&format!(
                    "to forget it, run ringleader delete {name} --forget from another terminal"
                )))
            }
            Err(error) => OperationOutcome::Failed(error),
        }
    }

    fn deleted(name: &str, outcome: &DeleteOutcome) -> OperationOutcome {
        let summary = outcome.summary(name);
        match outcome.stray_session() {
            Some(warning) => OperationOutcome::Warned {
                message: format!("{summary}; {warning}"),
                select: None,
            },
            None => OperationOutcome::done(summary, None),
        }
    }

    fn delete(service: &mut SessionService, name: String, seen: DeleteImpact) -> OperationOutcome {
        match service.delete_session(&name, &DeleteConfirmation::confirmed(seen)) {
            Ok(outcome) => Self::deleted(&name, &outcome),
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
    ConfirmDeleteSessions(BatchReport<DeleteImpact>),
    Opened {
        name: String,
        message: String,
    },
    Failed(ServiceError),
}

impl OperationOutcome {
    pub fn done(message: String, select: Option<String>) -> Self {
        Self::Done { message, select }
    }

    pub fn reported<T>(report: &BatchReport<T>, past_tense: &str) -> Self {
        Self::batch(report.summary(past_tense), report.has_failures())
    }

    pub fn batch(message: String, needs_attention: bool) -> Self {
        match needs_attention {
            true => Self::Warned {
                message,
                select: None,
            },
            false => Self::done(message, None),
        }
    }

    fn started(message: String, outcome: &StartOutcome) -> Self {
        let select = Some(outcome.session().name().to_string());
        match outcome.warning() {
            Some(warning) => Self::Warned {
                message: format!("{message} But {warning}."),
                select,
            },
            None => Self::done(message, select),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::MergeOutcome;
    use crate::test_support::{FakeMultiplexer, ServiceFixture, UiHarness};

    struct MergeOperationFixture;

    impl MergeOperationFixture {
        const NAME: &'static str = "fix-auth";

        fn conflicted(multiplexer: FakeMultiplexer) -> ServiceFixture {
            let conflicts = MergeOutcome::Conflicted(vec!["src/auth.rs".to_string()]);
            ServiceFixture::merging(Self::NAME, conflicts, multiplexer)
        }

        fn run(fixture: &ServiceFixture) -> OperationOutcome {
            BlockingOperation::Merge(Self::NAME.to_string()).run(&mut fixture.service())
        }
    }

    struct StartOperationFixture;

    impl StartOperationFixture {
        fn warning(outcome: OperationOutcome) -> String {
            match outcome {
                OperationOutcome::Warned { message, .. } => message,
                other => panic!("expected a warning, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_create_whose_envrc_is_not_allowed_warns_after_reporting_the_session() {
        let fixture = ServiceFixture::new()
            .with_directory_environment(ServiceFixture::not_allowed_envrc("fix-auth"));
        let request = CreateRequest::new("fix-auth", ServiceFixture::repo());

        let outcome = BlockingOperation::Create(request).run(&mut fixture.service());

        let warning = StartOperationFixture::warning(outcome);
        assert!(
            warning.starts_with("Created fix-auth on branch fix-auth. But the .envrc of"),
            "{warning}"
        );
    }

    #[test]
    fn a_restart_whose_envrc_is_not_allowed_warns_after_reporting_the_restart() {
        let fixture = UiHarness::exited_session_fixture("fix-auth")
            .with_directory_environment(ServiceFixture::not_allowed_envrc("fix-auth"));

        let outcome =
            BlockingOperation::Restart("fix-auth".to_string()).run(&mut fixture.service());

        let warning = StartOperationFixture::warning(outcome);
        assert!(
            warning.starts_with("Restarted fix-auth. But the .envrc of"),
            "{warning}"
        );
    }

    #[test]
    fn a_first_measure_of_a_delete_reports_no_change_since_the_user_saw_it() {
        let session = ServiceFixture::session("fix-auth");
        let fixture = ServiceFixture::new()
            .with_sessions(std::slice::from_ref(&session))
            .with_workspace(ServiceFixture::workspace_for(&[&session]))
            .with_host(ServiceFixture::host_for(&[&session]));

        let outcome = BlockingOperation::Delete(DeleteStep::Measure("fix-auth".to_string()))
            .run(&mut fixture.service());

        assert!(
            matches!(
                outcome,
                OperationOutcome::ConfirmDelete { changed: false, .. }
            ),
            "{outcome:?}"
        );
    }

    #[test]
    fn opening_a_pull_request_announces_the_lookup_while_it_runs() {
        let operation = BlockingOperation::Open(BrowserPage::PullRequest("fix-auth".to_string()));

        assert_eq!(
            operation.busy_message(),
            "Finding the pull request of fix-auth…"
        );
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
        let fixture = MergeOperationFixture::conflicted(ServiceFixture::refusing_typing(
            MergeOperationFixture::NAME,
        ));

        let outcome = MergeOperationFixture::run(&fixture);

        assert!(
            matches!(outcome, OperationOutcome::Warned { .. }),
            "{outcome:?}"
        );
    }
}
