use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::ports::{Clock, HostProbe, ScratchSpace, ScratchSpaceError, ToolTempDirectory};
use super::service_ports::ServicePorts;
use super::tool_temp_claims::ToolTempClaims;
use crate::domain::Session;

pub struct ToolTempSweep {
    space: Arc<dyn ScratchSpace>,
    host: Arc<dyn HostProbe>,
    clock: Arc<dyn Clock>,
}

impl ToolTempSweep {
    pub fn new(ports: &ServicePorts) -> Self {
        Self {
            space: ports.state().scratch_space(),
            host: ports.repos().host(),
            clock: ports.state().clock(),
        }
    }

    pub fn retire_abandoned_everywhere(&self) {
        match self.space.directories() {
            Ok(directories) => self.retire_abandoned(directories),
            Err(error) => log::warn!("kept every tool temp directory: {error}"),
        }
    }

    pub fn retire_abandoned_in(&self, session: &Session) {
        self.retire_abandoned(vec![self.space.directory(session.worktree_path())]);
    }

    fn retire_abandoned(&self, directories: Vec<PathBuf>) {
        let listed = self.listed(directories);
        if listed.is_empty() {
            return;
        }
        let scanned: Vec<PathBuf> = listed.iter().map(|(scratch, _)| scratch.clone()).collect();
        let users = self.host.directory_users(&scanned);
        let now = self.clock.now_epoch_seconds();
        for (scratch, tool_temps) in &listed {
            let abandoned = ToolTempClaims::new(scratch, &users).abandoned(tool_temps, now);
            self.retire_each(scratch, &abandoned);
        }
    }

    fn listed(&self, directories: Vec<PathBuf>) -> Vec<(PathBuf, Vec<ToolTempDirectory>)> {
        directories
            .into_iter()
            .filter_map(|scratch| self.tool_temps_in(scratch))
            .filter(|(_, tool_temps)| !tool_temps.is_empty())
            .collect()
    }

    fn tool_temps_in(&self, scratch: PathBuf) -> Option<(PathBuf, Vec<ToolTempDirectory>)> {
        match self.space.tool_temp_directories(&scratch) {
            Ok(tool_temps) => Some((scratch, tool_temps)),
            Err(error) => {
                log::warn!("kept the tool temp directories: {error}");
                None
            }
        }
    }

    fn retire_each(&self, scratch: &Path, abandoned: &[PathBuf]) {
        let failures: Vec<ScratchSpaceError> = abandoned
            .iter()
            .filter_map(|tool_temp| self.space.retire_tool_temp(tool_temp).err())
            .collect();
        for error in &failures {
            log::warn!("kept an abandoned tool temp directory: {error}");
        }
        let retired = abandoned.len() - failures.len();
        if retired > 0 {
            log::info!(
                "retired abandoned tool temp directories from {}: {retired}",
                scratch.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::test_support::{FakeHostProbe, FakeScratchSpace, LogCapture, ServiceFixture};

    struct SweepFixture {
        service: ServiceFixture,
    }

    impl SweepFixture {
        const SESSION: &'static str = "fix-auth";
        const OTHER_SESSION: &'static str = "fix-ui";
        const BUILD: u32 = 4_000_311;

        fn with(space: FakeScratchSpace, host: FakeHostProbe) -> Self {
            let service = ServiceFixture::new()
                .with_scratch_space(space)
                .with_host(host);
            Self { service }
        }

        fn scratch(session: &str) -> PathBuf {
            FakeScratchSpace::default().directory(&ServiceFixture::worktree_path(session))
        }

        fn shell_in(session: &str, name: &str) -> ToolTempDirectory {
            ToolTempDirectory::ownerless(Self::scratch(session).join(name))
        }

        fn space_with(tool_temps: &[ToolTempDirectory]) -> FakeScratchSpace {
            FakeScratchSpace::default()
                .with_directories_of(&[
                    ServiceFixture::worktree_path(Self::SESSION),
                    ServiceFixture::worktree_path(Self::OTHER_SESSION),
                ])
                .with_tool_temps(tool_temps)
        }

        fn host_using(path: &Path) -> FakeHostProbe {
            FakeHostProbe::default().with_temp_user(Self::BUILD, "example-build", path)
        }

        fn sweep(&self) -> ToolTempSweep {
            ToolTempSweep::new(&self.service.ports())
        }

        fn retire_in_session(&self) {
            self.sweep()
                .retire_abandoned_in(&ServiceFixture::session(Self::SESSION));
        }

        fn retired(&self) -> Vec<PathBuf> {
            self.service.scratch_space().retired_tool_temps()
        }
    }

    #[test]
    fn a_sweep_of_a_session_retires_its_abandoned_tool_temp_directory() {
        let shell = SweepFixture::shell_in(SweepFixture::SESSION, "nix-shell.Ab12Cd");
        let fixture = SweepFixture::with(
            SweepFixture::space_with(std::slice::from_ref(&shell)),
            FakeHostProbe::default(),
        );

        fixture.retire_in_session();

        assert_eq!(fixture.retired(), [shell.path()]);
    }

    #[test]
    fn a_sweep_keeps_a_tool_temp_directory_that_a_process_uses() {
        let shell = SweepFixture::shell_in(SweepFixture::SESSION, "nix-shell.Ab12Cd");
        let fixture = SweepFixture::with(
            SweepFixture::space_with(std::slice::from_ref(&shell)),
            SweepFixture::host_using(shell.path()),
        );

        fixture.retire_in_session();

        assert!(fixture.retired().is_empty());
    }

    #[test]
    fn a_sweep_keeps_a_tool_temp_directory_modified_moments_ago() {
        let shell = SweepFixture::shell_in(SweepFixture::SESSION, "nix-shell.Qr90St")
            .modified_at(ServiceFixture::NOW);
        let fixture =
            SweepFixture::with(SweepFixture::space_with(&[shell]), FakeHostProbe::default());

        fixture.retire_in_session();

        assert!(fixture.retired().is_empty());
    }

    #[test]
    fn a_sweep_of_a_session_leaves_other_sessions_alone() {
        let other = SweepFixture::shell_in(SweepFixture::OTHER_SESSION, "nix-shell.Ef34Gh");
        let fixture =
            SweepFixture::with(SweepFixture::space_with(&[other]), FakeHostProbe::default());

        fixture.retire_in_session();

        assert!(fixture.retired().is_empty());
    }

    #[test]
    fn a_sweep_everywhere_retires_abandoned_tool_temp_directories_of_every_session() {
        let own = SweepFixture::shell_in(SweepFixture::SESSION, "nix-shell.Ab12Cd");
        let other = SweepFixture::shell_in(SweepFixture::OTHER_SESSION, "nix-shell.Ef34Gh");
        let fixture = SweepFixture::with(
            SweepFixture::space_with(&[own.clone(), other.clone()]),
            FakeHostProbe::default(),
        );

        fixture.sweep().retire_abandoned_everywhere();

        assert_eq!(fixture.retired(), [own.path(), other.path()]);
    }

    #[test]
    fn a_sweep_lists_tool_temp_directories_before_reading_processes_so_a_new_shell_has_its_user() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let shell = SweepFixture::shell_in(SweepFixture::SESSION, "nix-shell.Ab12Cd");
        let fixture = SweepFixture::with(
            SweepFixture::space_with(&[shell]).recording_tool_temp_listings(Arc::clone(&log)),
            FakeHostProbe::default().recording_directory_scans(Arc::clone(&log)),
        );

        fixture.retire_in_session();

        assert_eq!(*log.lock().unwrap(), ["list tool temps", "scan"]);
    }

    #[test]
    fn a_sweep_without_tool_temp_directories_reads_no_processes() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let fixture = SweepFixture::with(
            SweepFixture::space_with(&[]).recording_tool_temp_listings(Arc::clone(&log)),
            FakeHostProbe::default().recording_directory_scans(Arc::clone(&log)),
        );

        fixture.retire_in_session();

        assert_eq!(*log.lock().unwrap(), ["list tool temps"]);
    }

    #[test]
    fn a_failed_listing_logs_a_warning_and_retires_nothing() {
        let capture = LogCapture::installed();
        let shell = SweepFixture::shell_in(SweepFixture::SESSION, "nix-shell.Ij56Kl");
        let fixture = SweepFixture::with(
            SweepFixture::space_with(&[shell]).failing("list tool temps"),
            FakeHostProbe::default(),
        );

        fixture.retire_in_session();

        assert!(fixture.retired().is_empty());
        assert_eq!(
            capture.count_containing("kept the tool temp directories: cannot list tool temps"),
            1
        );
    }

    #[test]
    fn a_failed_retire_logs_a_warning() {
        let capture = LogCapture::installed();
        let shell = SweepFixture::shell_in(SweepFixture::SESSION, "nix-shell.Mn78Op");
        let fixture = SweepFixture::with(
            SweepFixture::space_with(&[shell]).failing("retire tool temp"),
            FakeHostProbe::default(),
        );

        fixture.retire_in_session();

        assert_eq!(
            capture.count_containing("kept an abandoned tool temp directory: cannot retire tool temp /scratch/fix-auth/nix-shell.Mn78Op"),
            1
        );
    }
}
