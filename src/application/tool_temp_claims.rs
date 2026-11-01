use std::path::{Path, PathBuf};

use super::ports::{DirectoryUser, ToolTempDirectory};

pub struct ToolTempClaims<'u> {
    scratch: &'u Path,
    users: Vec<&'u DirectoryUser>,
}

impl<'u> ToolTempClaims<'u> {
    const LAUNCH_GRACE_SECONDS: u64 = 600;

    pub fn new(scratch: &'u Path, users: &'u [DirectoryUser]) -> Self {
        Self {
            scratch,
            users: users.iter().filter(|user| user.works_in(scratch)).collect(),
        }
    }

    pub fn abandoned(&self, tool_temps: &[ToolTempDirectory], now: u64) -> Vec<PathBuf> {
        let ownerless_kept = self.has_unplaced_owner(tool_temps);
        tool_temps
            .iter()
            .filter(|tool_temp| !Self::may_be_launching(tool_temp, now))
            .filter(|tool_temp| match tool_temp.owner() {
                Some(_) => self.owner_of(tool_temp).is_none(),
                None => !ownerless_kept && !self.is_used(tool_temp.path()),
            })
            .map(|tool_temp| tool_temp.path().to_path_buf())
            .collect()
    }

    fn may_be_launching(tool_temp: &ToolTempDirectory, now: u64) -> bool {
        now.saturating_sub(tool_temp.modified_epoch_seconds()) < Self::LAUNCH_GRACE_SECONDS
    }

    fn has_unplaced_owner(&self, tool_temps: &[ToolTempDirectory]) -> bool {
        let ownerless: Vec<&Path> = tool_temps
            .iter()
            .filter(|tool_temp| tool_temp.owner().is_none())
            .map(ToolTempDirectory::path)
            .collect();
        tool_temps
            .iter()
            .filter_map(|tool_temp| self.owner_of(tool_temp))
            .any(|owner| !owner.works_only_in(&ownerless, self.scratch))
    }

    fn owner_of(&self, tool_temp: &ToolTempDirectory) -> Option<&DirectoryUser> {
        let owner = tool_temp.owner()?;
        self.users
            .iter()
            .copied()
            .find(|user| user.process().pid() == owner)
    }

    fn is_used(&self, path: &Path) -> bool {
        self.users.iter().any(|user| user.works_in(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::ProcessInfo;

    struct ClaimsFixture;

    impl ClaimsFixture {
        const SCRATCH: &'static str = "/scratch/8718940d";
        const OTHER_SCRATCH: &'static str = "/scratch/0c5a1e7b";
        const SHELL: &'static str = "/scratch/8718940d/nix-shell.Ab12Cd";
        const OTHER_SHELL: &'static str = "/scratch/8718940d/nix-shell.Ef34Gh";
        const LAUNCHER: &'static str = "/scratch/8718940d/nix-develop-4100-7";
        const OWNER: u32 = 4100;
        const STRANGER: u32 = 5200;
        const NOW: u64 = 1_700_000_100;

        fn user(pid: u32, path: &str) -> DirectoryUser {
            DirectoryUser::new(ProcessInfo::new(pid, "example-build"), vec![path.into()])
        }

        fn user_with(pid: u32, paths: &[&str]) -> DirectoryUser {
            let paths = paths.iter().map(PathBuf::from).collect();
            DirectoryUser::new(ProcessInfo::new(pid, "example-build"), paths)
        }

        fn shell(path: &str) -> ToolTempDirectory {
            ToolTempDirectory::ownerless(path.into())
        }

        fn launcher() -> ToolTempDirectory {
            ToolTempDirectory::owned(Self::LAUNCHER.into(), Self::OWNER)
        }

        fn abandoned(users: &[DirectoryUser], tool_temps: &[ToolTempDirectory]) -> Vec<PathBuf> {
            ToolTempClaims::new(Path::new(Self::SCRATCH), users).abandoned(tool_temps, Self::NOW)
        }
    }

    #[test]
    fn a_launcher_directory_whose_owner_has_exited_is_abandoned() {
        let abandoned = ClaimsFixture::abandoned(&[], &[ClaimsFixture::launcher()]);

        assert_eq!(abandoned, [PathBuf::from(ClaimsFixture::LAUNCHER)]);
    }

    #[test]
    fn a_launcher_directory_is_claimed_while_its_owner_works_in_the_scratch_directory() {
        let owner = ClaimsFixture::user(ClaimsFixture::OWNER, ClaimsFixture::SHELL);

        let abandoned = ClaimsFixture::abandoned(&[owner], &[ClaimsFixture::launcher()]);

        assert!(abandoned.is_empty(), "{abandoned:?}");
    }

    #[test]
    fn a_launcher_directory_is_abandoned_when_its_pid_now_runs_elsewhere_since_pids_are_reused() {
        let reused = ClaimsFixture::user(ClaimsFixture::OWNER, ClaimsFixture::OTHER_SCRATCH);

        let abandoned = ClaimsFixture::abandoned(&[reused], &[ClaimsFixture::launcher()]);

        assert_eq!(abandoned, [PathBuf::from(ClaimsFixture::LAUNCHER)]);
    }

    #[test]
    fn a_tool_temp_directory_modified_moments_ago_is_claimed_since_a_launch_makes_it_before_any_process_uses_it()
     {
        let launching =
            ClaimsFixture::shell(ClaimsFixture::SHELL).modified_at(ClaimsFixture::NOW - 1);

        let abandoned = ClaimsFixture::abandoned(&[], &[launching]);

        assert!(abandoned.is_empty(), "{abandoned:?}");
    }

    #[test]
    fn a_shell_directory_that_no_process_uses_is_abandoned() {
        let abandoned =
            ClaimsFixture::abandoned(&[], &[ClaimsFixture::shell(ClaimsFixture::SHELL)]);

        assert_eq!(abandoned, [PathBuf::from(ClaimsFixture::SHELL)]);
    }

    #[test]
    fn a_shell_directory_is_claimed_while_a_process_uses_a_path_inside_it() {
        let build = ClaimsFixture::user(ClaimsFixture::STRANGER, ClaimsFixture::SHELL);

        let abandoned =
            ClaimsFixture::abandoned(&[build], &[ClaimsFixture::shell(ClaimsFixture::SHELL)]);

        assert!(abandoned.is_empty(), "{abandoned:?}");
    }

    #[test]
    fn every_shell_directory_is_claimed_while_an_owner_works_outside_them_since_an_idle_shell_hides_its_own()
     {
        let idle_shell = ClaimsFixture::user(ClaimsFixture::OWNER, ClaimsFixture::SCRATCH);
        let tool_temps = [
            ClaimsFixture::launcher(),
            ClaimsFixture::shell(ClaimsFixture::SHELL),
        ];

        let abandoned = ClaimsFixture::abandoned(&[idle_shell], &tool_temps);

        assert!(abandoned.is_empty(), "{abandoned:?}");
    }

    #[test]
    fn every_shell_directory_is_claimed_while_an_idle_owner_inherits_a_path_into_another_shell_directory()
     {
        let idle_shell = ClaimsFixture::user_with(
            ClaimsFixture::OWNER,
            &[ClaimsFixture::SCRATCH, ClaimsFixture::OTHER_SHELL],
        );
        let tool_temps = [
            ClaimsFixture::launcher(),
            ClaimsFixture::shell(ClaimsFixture::SHELL),
            ClaimsFixture::shell(ClaimsFixture::OTHER_SHELL),
        ];

        let abandoned = ClaimsFixture::abandoned(&[idle_shell], &tool_temps);

        assert!(abandoned.is_empty(), "{abandoned:?}");
    }

    #[test]
    fn an_owner_working_inside_one_shell_directory_leaves_the_other_shell_directories_abandoned() {
        let owner = ClaimsFixture::user(ClaimsFixture::OWNER, ClaimsFixture::SHELL);
        let tool_temps = [
            ClaimsFixture::launcher(),
            ClaimsFixture::shell(ClaimsFixture::SHELL),
            ClaimsFixture::shell(ClaimsFixture::OTHER_SHELL),
        ];

        let abandoned = ClaimsFixture::abandoned(&[owner], &tool_temps);

        assert_eq!(abandoned, [PathBuf::from(ClaimsFixture::OTHER_SHELL)]);
    }
}
