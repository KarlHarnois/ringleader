use std::path::Path;
use std::sync::Arc;

use super::ports::{AgentLaunch, ScratchSpace};
use super::service_ports::ServicePorts;

pub struct SessionScratch {
    space: Arc<dyn ScratchSpace>,
}

impl SessionScratch {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            space: ports.state().scratch_space(),
        }
    }

    pub fn prepare(&self, launch: AgentLaunch) -> AgentLaunch {
        let worktree = launch.worktree().to_path_buf();
        match self.space.create(&worktree) {
            Ok(()) => launch.with_temp_directory(&self.space.directory(&worktree)),
            Err(error) => {
                log::warn!(
                    "{error}, so the agent in {} starts with the inherited temp directory",
                    worktree.display()
                );
                launch
            }
        }
    }

    pub fn discard(&self, worktree: &Path) {
        if let Err(error) = self.space.remove(worktree) {
            log::warn!(
                "{error}, so the scratch directory {} stays on disk, remove it by hand",
                self.space.directory(worktree).display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{FakeScratchSpace, LogCapture, ServiceFixture};

    struct ScratchFixture;

    impl ScratchFixture {
        fn launch(name: &str) -> AgentLaunch {
            let worktree = ServiceFixture::worktree_path(name);
            AgentLaunch::new(vec!["example-agent".to_string()], &[], &worktree)
        }

        fn scratch(space: FakeScratchSpace) -> (ServiceFixture, SessionScratch) {
            let fixture = ServiceFixture::new().with_scratch_space(space);
            let scratch = SessionScratch::new(&fixture.ports());
            (fixture, scratch)
        }
    }

    #[test]
    fn prepare_creates_the_directory_and_points_tmpdir_at_it() {
        let (fixture, scratch) = ScratchFixture::scratch(FakeScratchSpace::default());

        let launch = scratch.prepare(ScratchFixture::launch("fix-auth"));

        assert_eq!(
            launch.env_assignments(),
            [fixture.tmpdir_assignment("fix-auth")]
        );
        assert_eq!(
            fixture.scratch_space().created(),
            [ServiceFixture::worktree_path("fix-auth")]
        );
    }

    #[test]
    fn a_failed_prepare_keeps_the_launch_and_logs_a_warning() {
        let capture = LogCapture::installed();
        let (_fixture, scratch) =
            ScratchFixture::scratch(FakeScratchSpace::default().failing("create"));

        let launch = scratch.prepare(ScratchFixture::launch("prepare-fails"));

        assert_eq!(launch, ScratchFixture::launch("prepare-fails"));
        assert_eq!(capture.count_containing("prepare-fails starts with"), 1);
    }

    #[test]
    fn discard_removes_the_directory() {
        let (fixture, scratch) = ScratchFixture::scratch(FakeScratchSpace::default());
        let worktree = ServiceFixture::worktree_path("fix-auth");

        scratch.discard(&worktree);

        assert_eq!(fixture.scratch_space().removed(), [worktree]);
    }

    #[test]
    fn a_failed_discard_logs_a_warning() {
        let capture = LogCapture::installed();
        let (_fixture, scratch) =
            ScratchFixture::scratch(FakeScratchSpace::default().failing("remove"));

        scratch.discard(&ServiceFixture::worktree_path("discard-fails"));

        assert_eq!(capture.count_containing("discard-fails stays on disk"), 1);
    }
}
