use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::pending::Pending;
use super::ports::{HostProbe, ProcessInfo, ScratchSpace, SessionRegistry};
use super::scratch_claims::ScratchClaims;
use super::service_ports::ServicePorts;
use crate::domain::{Session, SessionObservation};

pub struct ScratchSweep {
    space: Arc<dyn ScratchSpace>,
    host: Arc<dyn HostProbe>,
    registry: Arc<dyn SessionRegistry>,
}

impl ScratchSweep {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            space: ports.state().scratch_space(),
            host: ports.repos().host(),
            registry: ports.state().registry(),
        }
    }

    pub fn retire_orphans(
        &self,
        deleted: &Session,
        entries: &[Session],
        observations: &[SessionObservation],
    ) {
        let claims = ScratchClaims::new(self.space.as_ref(), entries, observations);
        match self.space.directories() {
            Ok(directories) => {
                for orphan in claims.orphans(self.sweepable(deleted, directories)) {
                    self.retire_unless_in_use(&orphan);
                }
            }
            Err(error) => log::warn!("kept every orphaned scratch directory: {error}"),
        }
    }

    pub fn start_purge(&self) -> Pending<()> {
        let space = Arc::clone(&self.space);
        Pending::start(move || {
            Self::purge_each_retired(space.as_ref());
            Ok(())
        })
    }

    fn purge_each_retired(space: &dyn ScratchSpace) {
        match space.retired_directories() {
            Ok(directories) => {
                for directory in directories {
                    Self::purge(space, &directory);
                }
            }
            Err(error) => log::warn!("kept retired scratch directories on disk: {error}"),
        }
    }

    fn purge(space: &dyn ScratchSpace, directory: &Path) {
        if let Err(error) = space.purge(directory) {
            log::warn!("kept a retired scratch directory on disk: {error}");
        }
    }

    fn sweepable(&self, deleted: &Session, directories: Vec<PathBuf>) -> Vec<PathBuf> {
        match self.registry.moved_aside_copies() {
            Ok(copies) if copies.is_empty() => return directories,
            Ok(copies) => log::warn!(
                "kept every other orphaned scratch directory, since {} can still claim them",
                Self::listed(&copies)
            ),
            Err(error) => log::warn!("kept every other orphaned scratch directory: {error}"),
        }
        let own = self.space.directory(deleted.worktree_path());
        directories
            .into_iter()
            .filter(|directory| *directory == own)
            .collect()
    }

    fn listed(copies: &[PathBuf]) -> String {
        copies
            .iter()
            .map(|copy| copy.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn retire_unless_in_use(&self, orphan: &Path) {
        let users = self.host.processes_in(orphan, &[]);
        if users.is_empty() {
            self.retire(orphan);
        } else {
            log::info!(
                "kept the orphaned scratch directory {}, which {} still use",
                orphan.display(),
                ProcessInfo::listed(&users)
            );
        }
    }

    fn retire(&self, orphan: &Path) {
        match self.space.retire(orphan) {
            Ok(()) => log::info!(
                "retired the orphaned scratch directory {}",
                orphan.display()
            ),
            Err(error) => log::warn!("kept an orphaned scratch directory: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::test_support::{
        FakeHostProbe, FakeScratchSpace, FakeSessionRegistry, LogCapture, ServiceFixture,
    };

    struct SweepFixture {
        service: ServiceFixture,
    }

    impl SweepFixture {
        const ORPHAN: &'static str = "forgotten";
        const RECORDED: &'static str = "fix-auth";
        const DELETED: &'static str = "example-deleted";
        const FIRST_RETIRED: &'static str = "/scratch/.retired/sweep-first-0";
        const SECOND_RETIRED: &'static str = "/scratch/.retired/sweep-second-0";
        const ORPHAN_USER: u32 = 4_000_077;

        fn with_space(space: FakeScratchSpace) -> Self {
            Self::with(space, FakeHostProbe::default())
        }

        fn with(space: FakeScratchSpace, host: FakeHostProbe) -> Self {
            let service = ServiceFixture::new()
                .with_scratch_space(space)
                .with_host(host);
            Self { service }
        }

        fn with_moved_aside_registry(copy: &str) -> Self {
            Self::with_registry(FakeSessionRegistry::empty().with_moved_aside_copy(copy))
        }

        fn with_registry(registry: FakeSessionRegistry) -> Self {
            let service = ServiceFixture::new()
                .with_scratch_space(Self::space())
                .with_registry(registry);
            Self { service }
        }

        fn unpurged() -> FakeScratchSpace {
            FakeScratchSpace::default()
                .with_unpurged(Path::new(Self::FIRST_RETIRED))
                .with_unpurged(Path::new(Self::SECOND_RETIRED))
        }

        fn space() -> FakeScratchSpace {
            FakeScratchSpace::default().with_directories_of(&[
                ServiceFixture::worktree_path(Self::RECORDED),
                ServiceFixture::worktree_path(Self::ORPHAN),
            ])
        }

        fn orphan_directory(&self) -> PathBuf {
            self.service
                .scratch_space()
                .directory(&ServiceFixture::worktree_path(Self::ORPHAN))
        }

        fn host_with_a_process_in_the_orphan() -> FakeHostProbe {
            let orphan =
                FakeScratchSpace::default().directory(&ServiceFixture::worktree_path(Self::ORPHAN));
            FakeHostProbe::default().with_process(Self::ORPHAN_USER, "example-build", &orphan, 0)
        }

        fn sweep(&self) -> ScratchSweep {
            ScratchSweep::new(&self.service.ports())
        }

        fn retire_orphans(&self) {
            self.retire_orphans_after(Self::DELETED);
        }

        fn retire_orphans_after(&self, deleted: &str) {
            let recorded = ServiceFixture::session(Self::RECORDED);
            let deleted = ServiceFixture::session(deleted);
            self.sweep().retire_orphans(&deleted, &[recorded], &[]);
        }
    }

    #[test]
    fn an_orphaned_directory_is_retired() {
        let fixture = SweepFixture::with_space(SweepFixture::space());

        fixture.retire_orphans();

        assert_eq!(
            fixture.service.scratch_space().retired(),
            [fixture.orphan_directory()]
        );
    }

    #[test]
    fn an_orphan_that_a_process_still_runs_in_is_kept() {
        let fixture = SweepFixture::with(
            SweepFixture::space(),
            SweepFixture::host_with_a_process_in_the_orphan(),
        );

        fixture.retire_orphans();

        assert!(fixture.service.scratch_space().retired().is_empty());
    }

    #[test]
    fn a_moved_aside_registry_keeps_other_orphans_because_it_can_hold_stopped_sessions() {
        let capture = LogCapture::installed();
        let fixture =
            SweepFixture::with_moved_aside_registry("/state/ringleader/sessions.json.corrupt-7");

        fixture.retire_orphans();

        assert!(fixture.service.scratch_space().retired().is_empty());
        assert_eq!(
            capture.count_containing("sessions.json.corrupt-7 can still claim them"),
            1
        );
    }

    #[test]
    fn a_failed_moved_aside_listing_keeps_other_orphans_since_a_copy_may_exist() {
        let capture = LogCapture::installed();
        let fixture =
            SweepFixture::with_registry(FakeSessionRegistry::empty().failing_moved_aside_listing());

        fixture.retire_orphans();

        assert!(fixture.service.scratch_space().retired().is_empty());
        assert_eq!(
            capture.count_containing(
                "kept every other orphaned scratch directory: cannot access /state/ringleader"
            ),
            1
        );
    }

    #[test]
    fn a_moved_aside_registry_still_retires_the_deleted_sessions_own_directory() {
        let fixture =
            SweepFixture::with_moved_aside_registry("/state/ringleader/sessions.json.corrupt-8");

        fixture.retire_orphans_after(SweepFixture::ORPHAN);

        assert_eq!(
            fixture.service.scratch_space().retired(),
            [fixture.orphan_directory()]
        );
    }

    #[test]
    fn a_failed_listing_retires_nothing_and_logs_a_warning() {
        let capture = LogCapture::installed();
        let fixture = SweepFixture::with_space(SweepFixture::space().failing("list"));

        fixture.retire_orphans();

        assert!(fixture.service.scratch_space().retired().is_empty());
        assert_eq!(
            capture.count_containing("cannot list the scratch directory /scratch"),
            1
        );
    }

    #[test]
    fn a_failed_retire_logs_a_warning() {
        let capture = LogCapture::installed();
        let fixture = SweepFixture::with_space(SweepFixture::space().failing("retire"));

        fixture.retire_orphans();

        assert_eq!(
            capture.count_containing("cannot retire the scratch directory /scratch/forgotten"),
            1
        );
    }

    #[test]
    fn a_purge_removes_each_retired_directory() {
        let fixture = SweepFixture::with_space(SweepFixture::unpurged());

        fixture.sweep().start_purge().wait_unless(|| false);

        assert_eq!(
            fixture.service.scratch_space().purged(),
            [
                PathBuf::from(SweepFixture::FIRST_RETIRED),
                PathBuf::from(SweepFixture::SECOND_RETIRED)
            ]
        );
    }

    #[test]
    fn a_failed_purge_logs_each_kept_directory_so_the_log_explains_each_one() {
        let capture = LogCapture::installed();
        let fixture = SweepFixture::with_space(SweepFixture::unpurged().failing("purge"));

        let outcome = fixture.sweep().start_purge().wait_unless(|| false);

        assert!(matches!(outcome, Some(Ok(()))));
        for retired in [SweepFixture::FIRST_RETIRED, SweepFixture::SECOND_RETIRED] {
            assert_eq!(capture.count_containing(retired), 1, "{retired}");
        }
    }

    #[test]
    fn a_failed_retired_listing_logs_a_warning_and_still_finishes() {
        let capture = LogCapture::installed();
        let fixture = SweepFixture::with_space(FakeScratchSpace::default().failing("list retired"));

        let outcome = fixture.sweep().start_purge().wait_unless(|| false);

        assert!(matches!(outcome, Some(Ok(()))));
        assert_eq!(
            capture.count_containing("kept retired scratch directories on disk"),
            1
        );
    }
}
