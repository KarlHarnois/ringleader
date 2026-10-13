use std::sync::Arc;

use super::ports::{
    HostProbe, MergeOutcome, Multiplexer, MultiplexerError, UpstreamMerge, Workspace,
};
use super::reconciliation::ManagedRow;
use super::repo_merge::RepoMerge;
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use super::worktree_inspection::WorktreeInspection;
use crate::domain::{Config, Session, SessionName, SessionState};

pub struct SessionMerge {
    workspace: Arc<dyn Workspace>,
    multiplexer: Arc<dyn Multiplexer>,
    host: Arc<dyn HostProbe>,
    inspection: WorktreeInspection,
}

impl SessionMerge {
    const RESOLUTION_REQUEST: &'static str = "Resolve each conflict keeping the intent of both sides, run the tests, then commit the merge.";

    const NAMED_CONFLICT_LIMIT: usize = 20;

    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            workspace: ports.repos().workspace(),
            multiplexer: ports.tmux().multiplexer(),
            host: ports.repos().host(),
            inspection: WorktreeInspection::new(ports),
        }
    }

    pub fn merge(&self, row: &ManagedRow, config: &Config) -> Result<MergeReport, ServiceError> {
        let session = row.session();
        self.refuse_unsafe_merge(row)?;
        let base_branch =
            RepoMerge::new(self.host.as_ref()).base_branch_of(session, config.repos());
        let worktree = session.worktree_path();
        let branch_on_origin = self.workspace.fetch_upstream(worktree, base_branch)?;
        self.refuse_unsafe_merge(row)?;
        let merge = self.workspace.merge_upstream(worktree, &branch_on_origin)?;
        let handoff = self.hand_off_conflicts(session, &merge);
        Ok(MergeReport::new(session.name(), merge, handoff))
    }

    fn refuse_unsafe_merge(&self, row: &ManagedRow) -> Result<(), ServiceError> {
        let session = row.session();
        Self::refuse_unless_waiting(row)?;
        self.refuse_unless_at_prompt(session)?;
        self.refuse_unclean_worktree(session)?;
        self.refuse_off_its_branch(session)
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

    fn refuse_unless_at_prompt(&self, session: &Session) -> Result<(), ServiceError> {
        if self.multiplexer.agent_at_prompt(session.tmux_name())? {
            return Ok(());
        }
        Err(ServiceError::Conflict(format!(
            "the agent of session {} is not at its prompt, answer its dialog, send or clear its draft, or leave copy mode before merging",
            session.name()
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
                "session {} has uncommitted changes, have its agent commit them before merging",
                session.name()
            )));
        }
        Ok(())
    }

    fn refuse_off_its_branch(&self, session: &Session) -> Result<(), ServiceError> {
        let entry = self.inspection.listed_entry(session)?;
        let checked_out = entry.as_ref().and_then(|entry| entry.branch());
        if checked_out == Some(session.branch()) {
            return Ok(());
        }
        Err(ServiceError::Conflict(format!(
            "the worktree of session {} is not on its branch {}, finish any rebase or bisect and check the branch out before merging",
            session.name(),
            session.branch()
        )))
    }

    fn hand_off_conflicts(&self, session: &Session, merge: &UpstreamMerge) -> ConflictHandoff {
        let MergeOutcome::Conflicted(paths) = merge.outcome() else {
            return ConflictHandoff::Unneeded;
        };
        let prompt = Self::conflict_prompt(merge.upstream(), paths);
        match self
            .multiplexer
            .type_if_ready(session.tmux_name(), &[prompt])
        {
            Ok(()) => ConflictHandoff::Asked,
            Err(error) => ConflictHandoff::Refused(error),
        }
    }

    fn conflict_prompt(upstream: &str, paths: &[String]) -> String {
        format!(
            "Merging {upstream} into this branch stopped on conflicts in {}. {}",
            Self::named_paths(paths),
            Self::RESOLUTION_REQUEST
        )
    }

    fn named_paths(paths: &[String]) -> String {
        let named: Vec<&str> = paths
            .iter()
            .take(Self::NAMED_CONFLICT_LIMIT)
            .map(String::as_str)
            .collect();
        match paths.len() - named.len() {
            0 => named.join(", "),
            unnamed => format!(
                "{} and {unnamed} more listed by git status",
                named.join(", ")
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeReport {
    name: SessionName,
    merge: UpstreamMerge,
    handoff: ConflictHandoff,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConflictHandoff {
    Unneeded,
    Asked,
    Refused(MultiplexerError),
}

impl MergeReport {
    fn new(name: &SessionName, merge: UpstreamMerge, handoff: ConflictHandoff) -> Self {
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::application::ports::{DirtyState, WorkspaceError, WorktreeEntry};
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

        fn ending(outcome: MergeOutcome) -> ServiceFixture {
            ServiceFixture::merging(Self::NAME, outcome, FakeMultiplexer::new())
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
    fn a_long_conflict_list_names_the_first_paths_and_counts_the_rest() {
        let paths: Vec<String> = (1..=25)
            .map(|number| format!("src/file{number}.rs"))
            .collect();
        let fixture = MergeFixture::ending(MergeOutcome::Conflicted(paths));

        MergeFixture::merge(&fixture).unwrap();

        let typed = fixture
            .multiplexer()
            .typed_if_ready(MergeFixture::TMUX_NAME)
            .concat();
        assert!(
            typed.contains("src/file20.rs and 5 more listed by git status."),
            "{typed}"
        );
        assert!(!typed.contains("src/file21.rs"), "{typed}");
    }

    #[test]
    fn conflicts_asked_of_the_agent_are_summarized() {
        let fixture = MergeFixture::conflicted();

        let report = MergeFixture::merge(&fixture).unwrap();

        assert_eq!(
            report.summary(),
            "merging origin/main into fix-auth stopped on conflicts in 2 files, asked its agent to finish the merge"
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
    fn an_agent_that_leaves_its_prompt_during_the_merge_is_reported() {
        let fixture = ServiceFixture::merging(
            MergeFixture::NAME,
            MergeFixture::conflicts(),
            ServiceFixture::refusing_typing(MergeFixture::NAME),
        );

        let report = MergeFixture::merge(&fixture).unwrap();

        assert!(report.needs_attention());
        assert_eq!(
            report.summary(),
            "merging origin/main into fix-auth stopped on conflicts in 2 files, and its agent was not asked to finish the merge: the agent in tmux session ringleader-fix-auth is not at its prompt"
        );
    }

    #[test]
    fn a_single_conflict_is_named_in_the_singular() {
        let fixture =
            MergeFixture::ending(MergeOutcome::Conflicted(vec!["src/auth.rs".to_string()]));

        let report = MergeFixture::merge(&fixture).unwrap();

        assert_eq!(
            report.summary(),
            "merging origin/main into fix-auth stopped on a conflict in 1 file, asked its agent to finish the merge"
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
    fn an_agent_away_from_its_prompt_is_refused_before_git_runs() {
        let fixture = MergeFixture::stopped().with_multiplexer(
            FakeMultiplexer::new()
                .with_live(ServiceFixture::waiting(&MergeFixture::session()))
                .with_agent_away_from_prompt(MergeFixture::TMUX_NAME),
        );

        let error = MergeFixture::merge(&fixture).unwrap_err();

        assert_eq!(
            error,
            ServiceError::Conflict(
                "the agent of session fix-auth is not at its prompt, answer its dialog, send or clear its draft, or leave copy mode before merging"
                    .to_string()
            )
        );
        assert!(!MergeFixture::merged_upstream(&fixture));
    }

    #[test]
    fn an_agent_that_leaves_its_prompt_during_the_fetch_is_refused_before_git_merges() {
        let fixture = MergeFixture::stopped().with_multiplexer(
            FakeMultiplexer::new()
                .with_live(ServiceFixture::waiting(&MergeFixture::session()))
                .with_prompt_checks(MergeFixture::TMUX_NAME, &[true, false]),
        );

        let error = MergeFixture::merge(&fixture).unwrap_err();

        assert!(matches!(error, ServiceError::Conflict(_)), "{error:?}");
        assert!(
            fixture
                .workspace()
                .calls()
                .iter()
                .any(|call| call.starts_with("fetch_upstream"))
        );
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
                "session fix-auth has uncommitted changes, have its agent commit them before merging"
                    .to_string()
            )
        );
        assert!(!MergeFixture::merged_upstream(&fixture));
    }

    #[test]
    fn a_worktree_off_its_branch_is_refused_before_git_merges() {
        let worktree = MergeFixture::session().worktree_path().to_path_buf();
        let repo = Path::new(ServiceFixture::REPO);
        let detached = FakeWorkspace::default().with_worktree(
            repo,
            WorktreeEntry::new(&worktree).at_head(ServiceFixture::TIP),
        );
        let fixture = MergeFixture::waiting().with_workspace(detached);

        let error = MergeFixture::merge(&fixture).unwrap_err();

        assert_eq!(
            error,
            ServiceError::Conflict(
                "the worktree of session fix-auth is not on its branch fix-auth, finish any rebase or bisect and check the branch out before merging"
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
                .contains(&format!("fetch_upstream {worktree} develop"))
        );
    }

    #[test]
    fn a_failed_fetch_types_nothing() {
        let failure = WorkspaceError::Failed {
            subcommand: "fetch".to_string(),
            status: "exit status: 128".to_string(),
            stderr: "fatal: could not read from remote repository".to_string(),
        };
        let fixture = MergeFixture::waiting()
            .with_workspace(MergeFixture::workspace().failing("fetch_upstream", failure));

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
