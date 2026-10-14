use std::sync::Arc;

use super::merge_report::{ConflictHandoff, MergeReport, UpstreamMerge};
use super::ports::{HostProbe, MergeOutcome, Multiplexer, Workspace};
use super::reconciliation::{ManagedRow, Reconciliation};
use super::repo_merge::RepoMerge;
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use super::session_monitor::SessionMonitor;
use super::upstream_resolver::UpstreamResolver;
use super::worktree_inspection::WorktreeInspection;
use crate::domain::{BaseBranch, Config, Session, SessionState};

pub struct SessionMerge {
    workspace: Arc<dyn Workspace>,
    multiplexer: Arc<dyn Multiplexer>,
    host: Arc<dyn HostProbe>,
    inspection: WorktreeInspection,
    monitor: SessionMonitor,
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
            monitor: SessionMonitor::new(ports),
        }
    }

    pub fn merge(
        &self,
        row: &ManagedRow,
        reconciliation: &Reconciliation,
        config: &Config,
    ) -> Result<MergeReport, ServiceError> {
        let session = row.session();
        self.refuse_unsafe_merge(row)?;
        let upstream = self.upstream_of(session, reconciliation, config)?;
        self.workspace
            .fetch_upstream(session.worktree_path(), &upstream)?;
        self.refuse_unsafe_merge_now(session, reconciliation, config)?;
        let merge = self.merge_fetched(session, &upstream)?;
        let handoff = self.hand_off_conflicts(session, &merge);
        Ok(MergeReport::new(session.name(), merge, handoff))
    }

    fn upstream_of(
        &self,
        session: &Session,
        reconciliation: &Reconciliation,
        config: &Config,
    ) -> Result<BaseBranch, ServiceError> {
        let repos = RepoMerge::new(self.host.as_ref());
        let session_branches =
            repos.session_branches_in(session.repo_path(), reconciliation.entries());
        UpstreamResolver::new(self.workspace.as_ref())
            .skipping(session_branches)
            .resolve(
                session.worktree_path(),
                repos.base_branch_of(session, config.repos()),
            )
    }

    fn merge_fetched(
        &self,
        session: &Session,
        upstream: &BaseBranch,
    ) -> Result<UpstreamMerge, ServiceError> {
        let outcome = self.workspace.merge_upstream(
            session.worktree_path(),
            upstream,
            session.base_commit(),
        )?;
        Ok(UpstreamMerge::new(upstream, outcome))
    }

    fn refuse_unsafe_merge_now(
        &self,
        session: &Session,
        reconciliation: &Reconciliation,
        config: &Config,
    ) -> Result<(), ServiceError> {
        let current = self.monitor.reconcile(reconciliation.entries(), config)?;
        self.refuse_unsafe_merge(&current.managed(session.name().as_str())?)
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::application::ports::{DirtyState, WorkspaceError, WorktreeEntry};
    use crate::domain::{Branch, Repo, RepoName, SessionObservation};
    use crate::test_support::{FakeMultiplexer, FakeWorkspace, ServiceFixture};

    struct MergeFixture;

    impl MergeFixture {
        const NAME: &'static str = "fix-auth";
        const TMUX_NAME: &'static str = "ringleader-fix-auth";

        fn session() -> Session {
            ServiceFixture::session(Self::NAME)
        }

        fn stopped() -> ServiceFixture {
            Self::stopped_as(Self::session())
        }

        fn stopped_as(session: Session) -> ServiceFixture {
            ServiceFixture::new()
                .with_sessions(std::slice::from_ref(&session))
                .with_workspace(ServiceFixture::workspace_for(&[&session]))
                .with_host(ServiceFixture::host_for(&[&session]))
        }

        fn observed(observation: SessionObservation) -> ServiceFixture {
            Self::stopped().with_multiplexer(FakeMultiplexer::new().with_live(observation))
        }

        fn waiting() -> ServiceFixture {
            Self::waiting_as(Self::session())
        }

        fn waiting_as(session: Session) -> ServiceFixture {
            let observation = ServiceFixture::waiting(&session);
            Self::stopped_as(session)
                .with_multiplexer(FakeMultiplexer::new().with_live(observation))
        }

        fn config_with_develop() -> Config {
            let repo = Repo::explicit(
                RepoName::parse("example-project").unwrap(),
                Path::new(ServiceFixture::REPO),
                BaseBranch::new("develop"),
            );
            ServiceFixture::config().with_repos(vec![repo])
        }

        fn worktree() -> String {
            Self::session().worktree_path().display().to_string()
        }

        fn fetched_branch(fixture: &ServiceFixture, branch: &str) -> bool {
            let fetch = format!("fetch_upstream {} {branch}", Self::worktree());
            fixture.workspace().calls().contains(&fetch)
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

        fn fetched_upstream(fixture: &ServiceFixture) -> bool {
            fixture
                .workspace()
                .calls()
                .iter()
                .any(|call| call.starts_with("fetch_upstream"))
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
        assert!(MergeFixture::fetched_upstream(&fixture));
        assert!(!MergeFixture::merged_upstream(&fixture));
    }

    #[test]
    fn an_agent_that_starts_working_during_the_fetch_is_refused_before_git_merges() {
        let session = MergeFixture::session();
        let fixture = MergeFixture::stopped().with_multiplexer(
            FakeMultiplexer::new()
                .with_live(ServiceFixture::waiting(&session))
                .with_observation_after_next_look(ServiceFixture::working(&session)),
        );

        let error = MergeFixture::merge(&fixture).unwrap_err();

        assert!(matches!(error, ServiceError::Conflict(_)), "{error:?}");
        assert!(MergeFixture::fetched_upstream(&fixture));
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
    fn without_a_base_branch_origins_main_is_merged() {
        let fixture = MergeFixture::waiting();

        MergeFixture::merge(&fixture).unwrap();

        assert!(MergeFixture::fetched_branch(&fixture, "main"));
    }

    #[test]
    fn another_sessions_branch_named_main_is_never_merged() {
        let session = MergeFixture::session();
        let worktree = session.worktree_path().to_path_buf();
        let both_on_origin =
            ["main", "master"]
                .iter()
                .fold(MergeFixture::workspace(), |workspace, name| {
                    workspace.with_branch_tip(
                        &worktree,
                        Branch::origin_copy_of(name),
                        FakeWorkspace::DEFAULT_START,
                    )
                });
        let fixture = MergeFixture::waiting()
            .with_sessions(&[session, ServiceFixture::session("main")])
            .with_workspace(both_on_origin);

        MergeFixture::merge(&fixture).unwrap();

        assert!(MergeFixture::fetched_branch(&fixture, "master"));
    }

    #[test]
    fn the_configured_base_branch_is_merged() {
        let fixture = MergeFixture::waiting().with_config(MergeFixture::config_with_develop());

        MergeFixture::merge(&fixture).unwrap();

        assert!(MergeFixture::fetched_branch(&fixture, "develop"));
        assert!(fixture.workspace().calls().contains(&format!(
            "merge_upstream {} develop {}",
            MergeFixture::worktree(),
            FakeWorkspace::DEFAULT_START
        )));
    }

    #[test]
    fn the_report_names_origins_copy_of_the_merged_branch() {
        let fixture = MergeFixture::waiting().with_config(MergeFixture::config_with_develop());

        let report = MergeFixture::merge(&fixture).unwrap();

        assert_eq!(report.summary(), "merged origin/develop into fix-auth");
    }

    #[test]
    fn a_requested_base_branch_is_merged_over_the_configured_one() {
        let requested = ServiceFixture::session_based_on(MergeFixture::NAME, "release");
        let fixture =
            MergeFixture::waiting_as(requested).with_config(MergeFixture::config_with_develop());

        MergeFixture::merge(&fixture).unwrap();

        assert!(MergeFixture::fetched_branch(&fixture, "release"));
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
