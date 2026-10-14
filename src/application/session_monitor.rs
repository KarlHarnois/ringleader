use std::path::PathBuf;
use std::sync::Arc;

use super::diff_base_resolver::DiffBaseResolver;
use super::pending::Pending;
use super::ports::{Clock, HostProbe, Multiplexer, StartPoint, Workspace};
use super::reconciliation::Reconciliation;
use super::repo_merge::RepoMerge;
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use super::session_reconciler::SessionReconciler;
use crate::domain::{
    BaseBranch, Branch, Config, DiffStat, KeyBindings, Session, SessionObservation,
};

pub struct SessionMonitor {
    multiplexer: Arc<dyn Multiplexer>,
    diff_workspace: Arc<dyn Workspace>,
    host: Arc<dyn HostProbe>,
    clock: Arc<dyn Clock>,
}

impl SessionMonitor {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            multiplexer: ports.tmux().multiplexer(),
            diff_workspace: ports.repos().diff_workspace(),
            host: ports.repos().host(),
            clock: ports.state().clock(),
        }
    }

    pub fn observe_all(&self) -> Result<Vec<SessionObservation>, ServiceError> {
        Ok(self.multiplexer.observe_all()?)
    }

    pub fn start_observing(&self) -> Pending<Vec<SessionObservation>> {
        let multiplexer = Arc::clone(&self.multiplexer);
        Pending::start(move || Ok(multiplexer.observe_all()?))
    }

    pub fn reconcile(
        &self,
        entries: &[Session],
        config: &Config,
        observations: Vec<SessionObservation>,
    ) -> Reconciliation {
        let now = self.clock.now_epoch_seconds();
        let reconciler = SessionReconciler::new(config, self.host.as_ref(), now);
        reconciler.reconcile(entries, observations)
    }

    pub fn diff_stat(
        &self,
        session: &Session,
        sessions: &[Session],
        config: &Config,
    ) -> Result<DiffStat, ServiceError> {
        self.diff_measurement(session, sessions, config).measure()
    }

    pub fn start_diff(
        &self,
        session: &Session,
        sessions: &[Session],
        config: &Config,
    ) -> Pending<DiffStat> {
        let measurement = self.diff_measurement(session, sessions, config);
        Pending::start(move || measurement.measure())
    }

    pub fn start_preview(&self, tmux_name: &str) -> Pending<String> {
        let multiplexer = Arc::clone(&self.multiplexer);
        let tmux_name = tmux_name.to_string();
        Pending::start(move || Ok(multiplexer.capture_pane(&tmux_name)?))
    }

    pub fn server_key_bindings(&self) -> Option<KeyBindings> {
        let running = self
            .multiplexer
            .server_config(&KeyBindings::default())
            .ok()??;
        running.key_bindings().cloned()
    }

    fn diff_measurement(
        &self,
        session: &Session,
        sessions: &[Session],
        config: &Config,
    ) -> DiffMeasurement {
        let repos = RepoMerge::new(self.host.as_ref());
        DiffMeasurement {
            workspace: Arc::clone(&self.diff_workspace),
            worktree: session.worktree_path().to_path_buf(),
            recorded: StartPoint::from_base_commit(session.base_commit()),
            base_branch: repos.base_branch_of(session, config.repos()).cloned(),
            session_branches: repos.session_branches_in(session.repo_path(), sessions),
        }
    }
}

struct DiffMeasurement {
    workspace: Arc<dyn Workspace>,
    worktree: PathBuf,
    recorded: StartPoint,
    base_branch: Option<BaseBranch>,
    session_branches: Vec<Branch>,
}

impl DiffMeasurement {
    fn measure(self) -> Result<DiffStat, ServiceError> {
        let measured_from = DiffBaseResolver::new(self.workspace.as_ref())
            .skipping(self.session_branches)
            .resolve(&self.worktree, &self.recorded, self.base_branch.as_ref())?;
        Ok(self.workspace.diff_stat(&self.worktree, &measured_from)?)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::application::ports::StartPoint;
    use crate::domain::{BaseBranch, BaseBranchLadder, Branch, Config, DiffStat, Repo, RepoName};
    use crate::test_support::{FakeWorkspace, ServiceFixture};

    struct MonitorFixture;

    impl MonitorFixture {
        const SESSION: &'static str = "alpha";
        const BASE_BRANCH: &'static str = "develop";
        const SYMLINKED_REPO: &'static str = "/links/example-project";
        const OTHER_REPO: &'static str = "/repos/other-project";

        fn fixture_with_repo_at(repo_path: &str) -> ServiceFixture {
            let session = ServiceFixture::session(Self::SESSION);
            ServiceFixture::new()
                .with_sessions(&[session])
                .with_config(Self::config_with_repo_at(repo_path))
        }

        fn config_with_repo_at(repo_path: &str) -> Config {
            let repo = Repo::explicit(
                RepoName::parse("pinned").unwrap(),
                Path::new(repo_path),
                BaseBranch::new(Self::BASE_BRANCH),
            );
            ServiceFixture::config().with_repos(vec![repo])
        }

        fn measure(fixture: &ServiceFixture) {
            fixture.service().diff_stats(&[Self::SESSION.to_string()]);
        }

        fn measure_in_the_background(fixture: &ServiceFixture) {
            let measurement = fixture.service().start_diff(Self::SESSION).unwrap();
            measurement.wait_unless(|| false);
        }

        fn branch_lookups(fixture: &ServiceFixture) -> Vec<Vec<Branch>> {
            Self::measure(fixture);
            Self::recorded_branch_lookups(fixture)
        }

        fn background_branch_lookups(fixture: &ServiceFixture) -> Vec<Vec<Branch>> {
            Self::measure_in_the_background(fixture);
            Self::recorded_branch_lookups(fixture)
        }

        fn recorded_branch_lookups(fixture: &ServiceFixture) -> Vec<Vec<Branch>> {
            let requests = fixture.workspace().branch_tip_requests();
            requests.into_iter().map(|(_, branches)| branches).collect()
        }

        fn configured_lookups() -> Vec<Vec<Branch>> {
            let base_branch = BaseBranch::new(Self::BASE_BRANCH).unwrap();
            vec![base_branch.branches_and_origin_copy()]
        }
    }

    #[test]
    fn the_base_branch_configured_for_the_session_repo_reaches_the_diff() {
        let fixture = MonitorFixture::fixture_with_repo_at(ServiceFixture::REPO);

        assert_eq!(
            MonitorFixture::branch_lookups(&fixture),
            MonitorFixture::configured_lookups()
        );
    }

    #[test]
    fn the_background_diff_uses_the_configured_base_branch_too() {
        let fixture = MonitorFixture::fixture_with_repo_at(ServiceFixture::REPO);

        assert_eq!(
            MonitorFixture::background_branch_lookups(&fixture),
            MonitorFixture::configured_lookups()
        );
    }

    #[test]
    fn a_repo_configured_through_a_symlink_still_gives_its_base_branch() {
        let host = ServiceFixture::host_for(&[]).with_canonical(
            Path::new(MonitorFixture::SYMLINKED_REPO),
            Path::new(ServiceFixture::REPO),
        );
        let fixture =
            MonitorFixture::fixture_with_repo_at(MonitorFixture::SYMLINKED_REPO).with_host(host);

        assert_eq!(
            MonitorFixture::branch_lookups(&fixture),
            MonitorFixture::configured_lookups()
        );
    }

    #[test]
    fn a_base_branch_configured_for_another_repo_is_not_used() {
        let fixture = MonitorFixture::fixture_with_repo_at(MonitorFixture::OTHER_REPO);

        assert_eq!(
            MonitorFixture::branch_lookups(&fixture),
            [BaseBranchLadder::new(None).diff_lookups()]
        );
    }

    #[test]
    fn the_diff_is_measured_through_the_diff_poll_workspace() {
        let worktree = ServiceFixture::worktree_path(MonitorFixture::SESSION);
        let diff = DiffStat::from_changes([], 4);
        let fixture = MonitorFixture::fixture_with_repo_at(ServiceFixture::REPO)
            .with_diff_workspace(FakeWorkspace::default().with_diff(&worktree, diff));

        let measured = fixture
            .service()
            .diff_stats(&[MonitorFixture::SESSION.to_string()]);

        assert_eq!(measured[0].1, Ok(diff));
    }

    #[test]
    fn the_diff_measures_from_the_merge_base_it_finds() {
        let merge_base = "6666666666666666666666666666666666666666";
        let worktree = ServiceFixture::worktree_path(MonitorFixture::SESSION);
        let fixture = MonitorFixture::fixture_with_repo_at(ServiceFixture::REPO)
            .with_workspace(FakeWorkspace::default().with_merge_bases(&worktree, &[merge_base]));

        MonitorFixture::measure(&fixture);

        assert_eq!(
            fixture.workspace().diff_bases(),
            [StartPoint::Commit(merge_base.to_string())]
        );
    }

    #[test]
    fn another_sessions_branch_named_main_is_not_measured_with() {
        let master_tip = "8888888888888888888888888888888888888888";
        let session = ServiceFixture::session(MonitorFixture::SESSION);
        let worktree = ServiceFixture::worktree_path(MonitorFixture::SESSION);
        let workspace = FakeWorkspace::default()
            .with_branch_tip(
                &worktree,
                Branch::Local("main".to_string()),
                FakeWorkspace::DEFAULT_START,
            )
            .with_branch_tip(&worktree, Branch::Local("master".to_string()), master_tip);
        let fixture = ServiceFixture::new()
            .with_sessions(&[session.clone(), ServiceFixture::session("main")])
            .with_workspace(workspace);

        MonitorFixture::measure(&fixture);

        assert_eq!(
            fixture.workspace().merge_base_requests(),
            [vec![
                session.base_commit().to_string(),
                master_tip.to_string()
            ]]
        );
    }

    #[test]
    fn a_requested_base_branch_wins_over_the_configured_one() {
        let session = ServiceFixture::session_based_on(MonitorFixture::SESSION, "release");
        let fixture = ServiceFixture::new()
            .with_sessions(&[session])
            .with_config(MonitorFixture::config_with_repo_at(ServiceFixture::REPO));
        let release = BaseBranch::new("release").unwrap();

        assert_eq!(
            MonitorFixture::branch_lookups(&fixture),
            [release.branches_and_origin_copy()]
        );
    }
}
