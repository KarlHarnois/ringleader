use std::path::PathBuf;
use std::ptr;

use super::argument_area::ArgumentArea;
use super::errno::Errno;
use super::launchd_daemon::LaunchdDaemon;
use super::live_process::LiveProcess;
use super::nul_terminated::NulTerminated;

pub struct LibprocProcessTable;

impl LibprocProcessTable {
    const PID_HEADROOM: usize = 64;
    const PATH_SIZE: usize = libc::PROC_PIDPATHINFO_MAXSIZE as usize;
    const ARGUMENT_AREA_NAME_LENGTH: libc::c_uint = 3;
    const INCLUDING_ZOMBIES: u64 = 1;
    const NO_ARGUMENT: u64 = 0;

    pub fn pids() -> Vec<u32> {
        let mut pids = vec![0; Self::pid_count() + Self::PID_HEADROOM];
        let listed = Self::list_pids(&mut pids);
        pids.truncate(listed);
        pids.into_iter()
            .filter_map(|pid| u32::try_from(pid).ok())
            .collect()
    }

    pub fn is_alive(pid: u32) -> bool {
        libc::c_int::try_from(pid)
            .ok()
            .and_then(Self::short_info)
            .is_some_and(|info| info.pbsi_status != libc::SZOMB)
    }

    pub fn read(pid: u32) -> Option<LiveProcess> {
        let pid = libc::c_int::try_from(pid).ok()?;
        Self::short_info(pid).map(|info| Self::described(pid, &info))
    }

    #[cfg(test)]
    pub fn environment(pid: u32) -> Vec<String> {
        let pid = libc::c_int::try_from(pid).unwrap_or_default();
        ArgumentArea::new(Self::argument_area(pid)).environment()
    }

    fn described(pid: libc::c_int, info: &libc::proc_bsdshortinfo) -> LiveProcess {
        let process = LiveProcess::new(
            info.pbsi_pid,
            info.pbsi_ppid,
            Self::session(pid),
            info.pbsi_uid,
        )
        .with_cwd(Self::cwd(pid))
        .with_executable(&Self::executable(pid))
        .with_arguments(ArgumentArea::new(Self::argument_area(pid)).arguments());
        let server_shape = LaunchdDaemon::matches(&process);
        process.with_server_shape(server_shape)
    }

    fn pid_count() -> usize {
        // SAFETY: a null buffer asks only for the count, so proc_listallpids writes nothing.
        let count = unsafe { libc::proc_listallpids(ptr::null_mut(), 0) };
        usize::try_from(count).unwrap_or_default()
    }

    fn list_pids(pids: &mut [libc::pid_t]) -> usize {
        let size = libc::c_int::try_from(size_of_val(pids)).unwrap_or_default();
        // SAFETY: proc_listallpids writes at most `size` bytes, the size of `pids`.
        let listed = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), size) };
        usize::try_from(listed).unwrap_or_default().min(pids.len())
    }

    fn short_info(pid: libc::c_int) -> Option<libc::proc_bsdshortinfo> {
        Self::pid_info(pid, libc::PROC_PIDT_SHORTBSDINFO, Self::INCLUDING_ZOMBIES)
    }

    fn cwd(pid: libc::c_int) -> Option<PathBuf> {
        Self::pid_info::<libc::proc_vnodepathinfo>(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            Self::NO_ARGUMENT,
        )
        .and_then(|info| NulTerminated::path(info.pvi_cdir.vip_path.as_flattened()))
    }

    fn pid_info<T: Default>(pid: libc::c_int, flavor: libc::c_int, argument: u64) -> Option<T> {
        let mut info = T::default();
        let size = libc::c_int::try_from(size_of::<T>()).ok()?;
        // SAFETY: proc_pidinfo writes at most `size` bytes, the size of the T on this stack
        // frame. Each caller names the libc struct of its flavor, which holds only integers and
        // C characters, so any bytes the kernel writes form a valid T.
        let written =
            unsafe { libc::proc_pidinfo(pid, flavor, argument, (&raw mut info).cast(), size) };
        (written == size).then_some(info)
    }

    fn session(pid: libc::c_int) -> u32 {
        // SAFETY: getsid only reads the session id of a pid and touches no memory.
        let session = unsafe { libc::getsid(pid) };
        u32::try_from(session).unwrap_or_default()
    }

    fn executable(pid: libc::c_int) -> String {
        let mut path = [libc::c_char::default(); Self::PATH_SIZE];
        // SAFETY: proc_pidpath writes at most PATH_SIZE bytes into the buffer on this stack
        // frame, which holds that many.
        unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), Self::PATH_SIZE as u32) };
        NulTerminated::text(&path)
    }

    fn argument_area(pid: libc::c_int) -> Vec<u8> {
        let mut area = vec![0; Self::argument_area_size(pid)];
        let mut size = area.len();
        let mut name = Self::argument_area_name(pid);
        // SAFETY: sysctl writes at most `size` bytes into `area`, which holds that many, and
        // writes the length it filled back into `size`.
        let status = unsafe {
            libc::sysctl(
                name.as_mut_ptr(),
                Self::ARGUMENT_AREA_NAME_LENGTH,
                area.as_mut_ptr().cast(),
                &mut size,
                ptr::null_mut(),
                0,
            )
        };
        area.truncate(size);
        Errno::result(status).map(|_| area).unwrap_or_default()
    }

    fn argument_area_size(pid: libc::c_int) -> usize {
        let mut size = 0;
        let mut name = Self::argument_area_name(pid);
        // SAFETY: a null buffer asks only for the size, which sysctl writes into `size`.
        unsafe {
            libc::sysctl(
                name.as_mut_ptr(),
                Self::ARGUMENT_AREA_NAME_LENGTH,
                ptr::null_mut(),
                &mut size,
                ptr::null_mut(),
                0,
            )
        };
        size
    }

    fn argument_area_name(pid: libc::c_int) -> [libc::c_int; 3] {
        [libc::CTL_KERN, libc::KERN_PROCARGS2, pid]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{Deadline, ProcessMarker};

    struct TableFixture;

    impl TableFixture {
        fn read_once_started(pid: u32) -> Option<LiveProcess> {
            let mut process = None;
            Deadline::for_background_work().wait_until(|| {
                process = LibprocProcessTable::read(pid);
                process
                    .as_ref()
                    .is_some_and(|process| !process.info().command().is_empty())
            });
            process
        }

        fn end(mut child: std::process::Child) {
            child.kill().unwrap();
            child.wait().unwrap();
        }
    }

    #[test]
    fn the_pids_hold_this_process() {
        assert!(LibprocProcessTable::pids().contains(&std::process::id()));
    }

    #[test]
    fn a_child_is_read_with_its_parent_and_arguments() {
        let child = ProcessMarker::new().sleeping_child();

        let process = TableFixture::read_once_started(child.id());
        TableFixture::end(child);

        let process = process.unwrap();
        assert_eq!(process.parent(), std::process::id());
        assert_eq!(process.info().command(), "sleep 30");
    }

    #[test]
    fn this_process_works_in_a_directory() {
        let process = LibprocProcessTable::read(std::process::id()).unwrap();

        assert!(process.cwd().is_some_and(|cwd| cwd.is_absolute()));
    }

    #[test]
    fn launchd_is_read_although_another_user_owns_it() {
        assert!(LibprocProcessTable::read(1).is_some());
    }

    #[test]
    fn a_waited_child_is_not_alive() {
        let child = ProcessMarker::new().sleeping_child();
        let pid = child.id();
        let alive_before = LibprocProcessTable::is_alive(pid);

        TableFixture::end(child);

        assert!(alive_before);
        assert!(!LibprocProcessTable::is_alive(pid));
    }

    #[test]
    fn the_environment_of_a_child_holds_what_it_started_with() {
        let marker = ProcessMarker::new();
        let child = marker.sleeping_child();

        let environment = LibprocProcessTable::environment(child.id());
        TableFixture::end(child);

        let (name, value) = marker.variable();
        let entry = format!("{}={}", name.to_string_lossy(), value.to_string_lossy());
        assert!(environment.contains(&entry));
    }
}
