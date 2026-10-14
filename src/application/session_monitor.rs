use std::sync::Arc;

use super::ports::{Clock, DiffBase, HostProbe, Multiplexer, Workspace};
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

    pub fn diff_stat(&self, session: &Session, config: &Config) -> Result<DiffStat, ServiceError> {
        let base_branch = self.configured_base_branch(session, config);
        let base = DiffBase::new(session.base_commit(), base_branch);
        Ok(self.workspace.diff_stat(session.worktree_path(), &base)?)
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

    fn configured_base_branch<'c>(
        &self,
        session: &Session,
        config: &'c Config,
    ) -> Option<&'c BaseBranch> {
        RepoMerge::new(self.host.as_ref()).base_branch_of(session, config.repos())
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
