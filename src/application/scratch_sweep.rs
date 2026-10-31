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

    pub fn retire_orphans(&self, entries: &[Session], observations: &[SessionObservation]) {
        match self.registry.moved_aside_copies() {
            Ok(copies) if copies.is_empty() => self.retire_unclaimed(entries, observations),
            Ok(copies) => log::warn!(
                "kept every orphaned scratch directory, since {} can still claim some",
                Self::listed(&copies)
            ),
            Err(error) => log::warn!("kept every orphaned scratch directory: {error}"),
        }
    }

    pub fn start_purge(&self) -> Pending<()> {
        let space = Arc::clone(&self.space);
        Pending::start(move || {
            if let Err(error) = space.purge_retired() {
                log::warn!("kept retired scratch directories on disk: {error}");
            }
            Ok(())
        })
    }

    fn listed(copies: &[PathBuf]) -> String {
        copies
            .iter()
            .map(|copy| copy.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn retire_unclaimed(&self, entries: &[Session], observations: &[SessionObservation]) {
        let claims = ScratchClaims::new(self.space.as_ref(), entries, observations);
        match self.space.directories() {
            Ok(directories) => {
                for orphan in claims.orphans(directories) {
                    self.retire_unless_in_use(&orphan);
                }
            }
            Err(error) => log::warn!("kept every orphaned scratch directory: {error}"),
        }
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

        fn with_moved_aside_registry() -> Self {
            let service = ServiceFixture::new()
                .with_scratch_space(Self::space())
                .with_registry(FakeSessionRegistry::empty().with_moved_aside_copy());
            Self { service }
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
            let recorded = ServiceFixture::session(Self::RECORDED);
            self.sweep().retire_orphans(&[recorded], &[]);
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
    fn a_moved_aside_registry_keeps_every_orphan_because_it_can_hold_stopped_sessions() {
        let capture = LogCapture::installed();
        let fixture = SweepFixture::with_moved_aside_registry();

        fixture.retire_orphans();

        assert!(fixture.service.scratch_space().retired().is_empty());
        assert_eq!(
            capture.count_containing("sessions.json.corrupt-1 can still claim some"),
            1
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
    fn a_purge_empties_the_retired_directories() {
        let fixture = SweepFixture::with_space(FakeScratchSpace::default());

        fixture.sweep().start_purge().wait_unless(|| false);

        assert_eq!(fixture.service.scratch_space().purges(), 1);
    }

    #[test]
    fn a_failed_purge_logs_a_warning_and_still_finishes() {
        let capture = LogCapture::installed();
        let fixture = SweepFixture::with_space(FakeScratchSpace::default().failing("purge"));

        let outcome = fixture.sweep().start_purge().wait_unless(|| false);

        assert!(matches!(outcome, Some(Ok(()))));
        assert_eq!(
            capture.count_containing("kept retired scratch directories on disk"),
            1
        );
    }
}
