use std::sync::Arc;

use super::diff_base_resolver::DiffBaseResolver;
use super::ports::{Clock, HostProbe, Multiplexer, StartPoint, Workspace};
use super::reconciliation::Reconciliation;
use super::repo_merge::RepoMerge;
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use super::session_reconciler::SessionReconciler;
use crate::domain::{BaseBranch, Config, DiffStat, KeyBindings, Session};

pub struct SessionMonitor {
    multiplexer: Arc<dyn Multiplexer>,
    workspace: Arc<dyn Workspace>,
    host: Arc<dyn HostProbe>,
    clock: Arc<dyn Clock>,
}

impl SessionMonitor {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            multiplexer: ports.tmux().multiplexer(),
            workspace: ports.repos().workspace(),
            host: ports.repos().host(),
            clock: ports.state().clock(),
        }
    }

    pub fn reconcile(
        &self,
        entries: &[Session],
        config: &Config,
    ) -> Result<Reconciliation, ServiceError> {
        let observations = self.multiplexer.observe_all()?;
        let now = self.clock.now_epoch_seconds();
        let reconciler = SessionReconciler::new(config, self.host.as_ref(), now);
        Ok(reconciler.reconcile(entries, observations))
    }

    pub fn diff_stat(
        &self,
        session: &Session,
        sessions: &[Session],
        config: &Config,
    ) -> Result<DiffStat, ServiceError> {
        let base_branch = self.base_branch(session, config);
        let recorded = StartPoint::from_base_commit(session.base_commit());
        let worktree = session.worktree_path();
        let session_branches =
            RepoMerge::new(self.host.as_ref()).session_branches_in(session.repo_path(), sessions);
        let measured_from = DiffBaseResolver::new(self.workspace.as_ref())
            .skipping(session_branches)
            .resolve(worktree, &recorded, base_branch)?;
        Ok(self.workspace.diff_stat(worktree, &measured_from)?)
    }

    pub fn preview(&self, tmux_name: &str) -> Result<String, ServiceError> {
        Ok(self.multiplexer.capture_pane(tmux_name)?)
    }

    pub fn server_key_bindings(&self) -> Option<KeyBindings> {
        let running = self
            .multiplexer
            .server_config(&KeyBindings::default())
            .ok()??;
        running.key_bindings().cloned()
    }

    fn base_branch<'a>(&self, session: &'a Session, config: &'a Config) -> Option<&'a BaseBranch> {
        RepoMerge::new(self.host.as_ref()).base_branch_of(session, config.repos())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::application::ports::StartPoint;
    use crate::domain::{BaseBranch, BaseBranchLadder, Branch, Config, Repo, RepoName};
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

        fn branch_lookups(fixture: &ServiceFixture) -> Vec<Vec<Branch>> {
            Self::measure(fixture);
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
