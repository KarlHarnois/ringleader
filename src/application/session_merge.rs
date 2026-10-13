use std::sync::Arc;

use super::ports::{
    HostProbe, MergeOutcome, Multiplexer, MultiplexerError, UpstreamMerge, Workspace,
};
use super::reconciliation::ManagedRow;
use super::repo_merge::RepoMerge;
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use crate::domain::{Config, Session, SessionName, SessionState};

pub struct SessionMerge {
    workspace: Arc<dyn Workspace>,
    multiplexer: Arc<dyn Multiplexer>,
    host: Arc<dyn HostProbe>,
}

impl SessionMerge {
    const RESOLUTION_REQUEST: &'static str = "Resolve each conflict keeping the intent of both sides, run the tests, then commit the merge.";

    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            workspace: ports.repos().workspace(),
            multiplexer: ports.tmux().multiplexer(),
            host: ports.repos().host(),
        }
    }

    pub fn merge(&self, row: &ManagedRow, config: &Config) -> Result<MergeReport, ServiceError> {
        let session = row.session();
        Self::refuse_unless_waiting(row)?;
        self.refuse_unclean_worktree(session)?;
        let base_branch =
            RepoMerge::new(self.host.as_ref()).base_branch_of(session, config.repos());
        let merge = self
            .workspace
            .merge_upstream(session.worktree_path(), base_branch)?;
        let handoff = self.hand_off_conflicts(session, &merge);
        Ok(MergeReport::new(session.name(), merge, handoff))
    }

    fn refuse_unless_waiting(row: &ManagedRow) -> Result<(), ServiceError> {
        if row.state() == SessionState::Waiting {
            return Ok(());
        }
        Err(ServiceError::Conflict(format!(
            "session {} is {}, merge only runs while its agent waits at its prompt",
            row.session().name(),
            row.state().label()
        )))
    }

    fn refuse_unclean_worktree(&self, session: &Session) -> Result<(), ServiceError> {
        let dirty = self.workspace.dirty_state(session.worktree_path())?;
        if dirty.is_missing() {
            return Err(ServiceError::Invalid(format!(
                "worktree {} is missing, run ringleader doctor",
                session.worktree_path().display()
            )));
        }
        if dirty.dirty_files() > 0 {
            return Err(ServiceError::Conflict(format!(
                "session {} has uncommitted changes, commit or stash them before merging",
                session.name()
            )));
        }
        Ok(())
    }

    fn hand_off_conflicts(&self, session: &Session, merge: &UpstreamMerge) -> AgentHandoff {
        let MergeOutcome::Conflicted(paths) = merge.outcome() else {
            return AgentHandoff::Unneeded;
        };
        let prompt = Self::conflict_prompt(merge.upstream(), paths);
        match self
            .multiplexer
            .type_if_ready(session.tmux_name(), &[prompt])
        {
            Ok(()) => AgentHandoff::Asked,
            Err(error) => AgentHandoff::Refused(error),
        }
    }

    fn conflict_prompt(upstream: &str, paths: &[String]) -> String {
        format!(
            "Merging {upstream} into this branch stopped on conflicts in {}. {}",
            paths.join(", "),
            Self::RESOLUTION_REQUEST
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeReport {
    name: SessionName,
    merge: UpstreamMerge,
    handoff: AgentHandoff,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AgentHandoff {
    Unneeded,
    Asked,
    Refused(MultiplexerError),
}

impl MergeReport {
    fn new(name: &SessionName, merge: UpstreamMerge, handoff: AgentHandoff) -> Self {
        Self {
            name: name.clone(),
            merge,
            handoff,
        }
    }

    pub fn needs_attention(&self) -> bool {
        matches!(self.handoff, AgentHandoff::Refused(_))
    }

    pub fn summary(&self) -> String {
        let name = &self.name;
        let upstream = self.merge.upstream();
        match (self.merge.outcome(), &self.handoff) {
            (MergeOutcome::UpToDate, _) => format!("{name} already has {upstream}"),
            (MergeOutcome::Merged, _) => format!("merged {upstream} into {name}"),
            (MergeOutcome::Conflicted(paths), AgentHandoff::Refused(error)) => format!(
                "merging {upstream} into {name} stopped on {}, and its agent was not asked to resolve them: {error}",
                Self::conflicts(paths)
            ),
            (MergeOutcome::Conflicted(paths), _) => format!(
                "merging {upstream} into {name} stopped on {}, asked its agent to resolve them",
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::application::ports::{DirtyState, WorkspaceError};
    use crate::domain::{BaseBranch, Repo, RepoName, SessionObservation};
    use crate::test_support::{FakeMultiplexer, FakeWorkspace, ServiceFixture};

    struct MergeFixture;

    impl MergeFixture {
        const NAME: &'static str = "fix-auth";
        const TMUX_NAME: &'static str = "ringleader-fix-auth";

        fn session() -> Session {
            ServiceFixture::session(Self::NAME)
        }

        fn stopped() -> ServiceFixture {
            let session = Self::session();
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&session))
                .with_workspace(ServiceFixture::workspace_for(&[&session]))
                .with_host(ServiceFixture::host_for(&[&session]))
        }

        fn observed(observation: SessionObservation) -> ServiceFixture {
            Self::stopped().with_multiplexer(FakeMultiplexer::new().with_live(observation))
        }

        fn waiting() -> ServiceFixture {
            Self::observed(ServiceFixture::waiting(&Self::session()))
        }

        fn with_failing_tmux(fixture: ServiceFixture, operation: &'static str) -> ServiceFixture {
            fixture.with_multiplexer(
                FakeMultiplexer::new()
                    .with_live(ServiceFixture::waiting(&Self::session()))
                    .failing(operation, Self::not_ready()),
            )
        }

        fn ending(outcome: MergeOutcome) -> ServiceFixture {
            Self::waiting().with_workspace(Self::workspace().with_upstream_merge(outcome))
        }

        fn workspace() -> FakeWorkspace {
            ServiceFixture::workspace_for(&[&Self::session()])
        }

        fn conflicted() -> ServiceFixture {
            Self::ending(Self::conflicts())
        }

        fn conflicts() -> MergeOutcome {
            MergeOutcome::Conflicted(vec!["src/auth.rs".to_string(), "README.md".to_string()])
        }

        fn merge(fixture: &ServiceFixture) -> Result<MergeReport, ServiceError> {
            fixture.service().merge_upstream(Self::NAME)
        }

        fn merged_upstream(fixture: &ServiceFixture) -> bool {
            fixture
                .workspace()
                .calls()
                .iter()
                .any(|call| call.starts_with("merge_upstream"))
        }

        fn not_ready() -> MultiplexerError {
            MultiplexerError::AgentNotReady {
                tmux_name: Self::TMUX_NAME.to_string(),
            }
        }
    }

    #[test]
    fn conflicts_ask_the_agent_to_resolve_them_in_one_line() {
        let fixture = MergeFixture::conflicted();

        MergeFixture::merge(&fixture).unwrap();

        assert_eq!(
            fixture
                .multiplexer()
                .typed_if_ready(MergeFixture::TMUX_NAME),
            [format!(
                "Merging origin/main into this branch stopped on conflicts in src/auth.rs, README.md. {}",
                SessionMerge::RESOLUTION_REQUEST
            )]
        );
    }

    #[test]
    fn conflicts_asked_of_the_agent_are_summarized() {
        let fixture = MergeFixture::conflicted();

        let report = MergeFixture::merge(&fixture).unwrap();

        assert_eq!(
            report.summary(),
            "merging origin/main into fix-auth stopped on conflicts in 2 files, asked its agent to resolve them"
        );
        assert!(!report.needs_attention());
    }

    #[test]
    fn a_clean_merge_types_nothing() {
        let fixture = MergeFixture::waiting();

        let report = MergeFixture::merge(&fixture).unwrap();

        assert_eq!(report.summary(), "merged origin/main into fix-auth");
        assert!(
            fixture
                .multiplexer()
                .typed_if_ready(MergeFixture::TMUX_NAME)
                .is_empty()
        );
    }

    #[test]
    fn an_up_to_date_session_is_summarized() {
        let fixture = MergeFixture::ending(MergeOutcome::UpToDate);

        let report = MergeFixture::merge(&fixture).unwrap();

        assert_eq!(report.summary(), "fix-auth already has origin/main");
    }

    #[test]
    fn an_agent_away_from_its_prompt_is_reported() {
        let fixture = MergeFixture::with_failing_tmux(MergeFixture::conflicted(), "type_if_ready");

        let report = MergeFixture::merge(&fixture).unwrap();

        assert!(report.needs_attention());
        assert_eq!(
            report.summary(),
            "merging origin/main into fix-auth stopped on conflicts in 2 files, and its agent was not asked to resolve them: the agent in tmux session ringleader-fix-auth is not at its prompt"
        );
    }

    #[test]
    fn a_single_conflict_is_named_in_the_singular() {
        let fixture =
            MergeFixture::ending(MergeOutcome::Conflicted(vec!["src/auth.rs".to_string()]));

        let report = MergeFixture::merge(&fixture).unwrap();

        assert_eq!(
            report.summary(),
            "merging origin/main into fix-auth stopped on a conflict in 1 file, asked its agent to resolve them"
        );
    }

    #[test]
    fn a_working_session_is_refused_before_git_runs() {
        let fixture = MergeFixture::observed(ServiceFixture::working(&MergeFixture::session()));

        let error = MergeFixture::merge(&fixture).unwrap_err();

        assert_eq!(
            error,
            ServiceError::Conflict(
                "session fix-auth is working, merge only runs while its agent waits at its prompt"
                    .to_string()
            )
        );
        assert!(!MergeFixture::merged_upstream(&fixture));
    }

    #[test]
    fn an_exited_session_is_refused_before_git_runs() {
        let fixture = MergeFixture::observed(ServiceFixture::exited(&MergeFixture::session()));

        let error = MergeFixture::merge(&fixture).unwrap_err();

        assert!(matches!(error, ServiceError::Conflict(_)), "{error:?}");
        assert!(!MergeFixture::merged_upstream(&fixture));
    }

    #[test]
    fn a_stopped_session_is_refused_before_git_runs() {
        let fixture = MergeFixture::stopped();

        let error = MergeFixture::merge(&fixture).unwrap_err();

        assert!(matches!(error, ServiceError::Conflict(_)), "{error:?}");
        assert!(!MergeFixture::merged_upstream(&fixture));
    }

    #[test]
    fn uncommitted_changes_are_refused_before_git_merges() {
        let worktree = MergeFixture::session().worktree_path().to_path_buf();
        let dirty =
            MergeFixture::workspace().with_dirty(&worktree, DirtyState::present(3, Vec::new()));
        let fixture = MergeFixture::waiting().with_workspace(dirty);

        let error = MergeFixture::merge(&fixture).unwrap_err();

        assert_eq!(
            error,
            ServiceError::Conflict(
                "session fix-auth has uncommitted changes, commit or stash them before merging"
                    .to_string()
            )
        );
        assert!(!MergeFixture::merged_upstream(&fixture));
    }

    #[test]
    fn a_missing_worktree_is_refused_before_git_merges() {
        let worktree = MergeFixture::session().worktree_path().to_path_buf();
        let missing = MergeFixture::workspace().with_dirty(&worktree, DirtyState::missing());
        let fixture = MergeFixture::waiting().with_workspace(missing);

        let error = MergeFixture::merge(&fixture).unwrap_err();

        assert!(matches!(error, ServiceError::Invalid(_)), "{error:?}");
        assert!(!MergeFixture::merged_upstream(&fixture));
    }

    #[test]
    fn the_configured_base_branch_is_merged() {
        let base = BaseBranch::new("develop");
        let repo = Repo::explicit(
            RepoName::parse("example-project").unwrap(),
            Path::new(ServiceFixture::REPO),
            base,
        );
        let fixture =
            MergeFixture::waiting().with_config(ServiceFixture::config().with_repos(vec![repo]));

        MergeFixture::merge(&fixture).unwrap();

        let worktree = MergeFixture::session()
            .worktree_path()
            .display()
            .to_string();
        assert!(
            fixture
                .workspace()
                .calls()
                .contains(&format!("merge_upstream {worktree} develop"))
        );
    }

    #[test]
    fn a_failed_merge_types_nothing() {
        let failure = WorkspaceError::Failed {
            subcommand: "fetch".to_string(),
            status: "exit status: 128".to_string(),
            stderr: "fatal: could not read from remote repository".to_string(),
        };
        let fixture = MergeFixture::waiting()
            .with_workspace(MergeFixture::workspace().failing("merge_upstream", failure));

        let error = MergeFixture::merge(&fixture).unwrap_err();

        assert!(matches!(error, ServiceError::Failed(_)), "{error:?}");
        assert!(
            fixture
                .multiplexer()
                .typed_if_ready(MergeFixture::TMUX_NAME)
                .is_empty()
        );
    }
}
