use std::sync::Arc;

use super::pending::Pending;
use super::ports::{Clock, DiffBase, HostProbe, Multiplexer, Workspace};
use super::reconciliation::Reconciliation;
use super::repo_merge::RepoMerge;
use super::service_error::ServiceError;
use super::service_ports::ServicePorts;
use super::session_reconciler::SessionReconciler;
use crate::domain::{BaseBranch, Config, DiffStat, KeyBindings, Repo, Session, SessionObservation};

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

    pub fn diff_stat(&self, session: &Session, config: &Config) -> Result<DiffStat, ServiceError> {
        let base = self.diff_base(session, config);
        Ok(self.workspace.diff_stat(session.worktree_path(), &base)?)
    }

    pub fn start_diff(&self, session: &Session, config: &Config) -> Pending<DiffStat> {
        let workspace = Arc::clone(&self.workspace);
        let worktree = session.worktree_path().to_path_buf();
        let base = self.diff_base(session, config);
        Pending::start(move || Ok(workspace.diff_stat(&worktree, &base)?))
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

    fn diff_base(&self, session: &Session, config: &Config) -> DiffBase {
        let base_branch = self.configured_base_branch(session, config);
        DiffBase::new(session.base_commit(), base_branch)
    }

    fn configured_base_branch<'c>(
        &self,
        session: &Session,
        config: &'c Config,
    ) -> Option<&'c BaseBranch> {
        let repo_path = self.host.canonical_or_given(session.repo_path());
        RepoMerge::new(self.host.as_ref())
            .configured(&repo_path, config.repos())
            .and_then(Repo::base_branch)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::domain::{BaseBranch, Config, Repo, RepoName};
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

        fn diff_calls(fixture: &ServiceFixture) -> Vec<String> {
            fixture.service().diff_stats(&[Self::SESSION.to_string()]);
            Self::recorded_diff_calls(fixture)
        }

        fn background_diff_calls(fixture: &ServiceFixture) -> Vec<String> {
            let measurement = fixture.service().start_diff(Self::SESSION).unwrap();
            measurement.wait_unless(|| false);
            Self::recorded_diff_calls(fixture)
        }

        fn recorded_diff_calls(fixture: &ServiceFixture) -> Vec<String> {
            let calls = fixture.workspace().calls();
            calls
                .into_iter()
                .filter(|call| call.starts_with("diff_stat"))
                .collect()
        }

        fn diff_call_against(base_branch: &str) -> String {
            let worktree = ServiceFixture::worktree_path(Self::SESSION);
            format!("diff_stat {} {base_branch}", worktree.display())
        }
    }

    #[test]
    fn the_base_branch_configured_for_the_session_repo_reaches_the_diff() {
        let fixture = MonitorFixture::fixture_with_repo_at(ServiceFixture::REPO);

        assert_eq!(
            MonitorFixture::diff_calls(&fixture),
            [MonitorFixture::diff_call_against(
                MonitorFixture::BASE_BRANCH
            )]
        );
    }

    #[test]
    fn the_background_diff_uses_the_configured_base_branch_too() {
        let fixture = MonitorFixture::fixture_with_repo_at(ServiceFixture::REPO);

        assert_eq!(
            MonitorFixture::background_diff_calls(&fixture),
            [MonitorFixture::diff_call_against(
                MonitorFixture::BASE_BRANCH
            )]
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
            MonitorFixture::diff_calls(&fixture),
            [MonitorFixture::diff_call_against(
                MonitorFixture::BASE_BRANCH
            )]
        );
    }

    #[test]
    fn a_base_branch_configured_for_another_repo_is_not_used() {
        let fixture = MonitorFixture::fixture_with_repo_at(MonitorFixture::OTHER_REPO);

        assert_eq!(
            MonitorFixture::diff_calls(&fixture),
            [MonitorFixture::diff_call_against(
                FakeWorkspace::NO_BASE_BRANCH
            )]
        );
    }
}
