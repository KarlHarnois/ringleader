use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::Child;
use std::ptr;
use std::time::{Duration, Instant};

use super::exit_queue::{ExitQueue, WaitReport};
use crate::adapters::host::errno::Errno;

pub struct KqueueExitWatch {
    queue: OwnedFd,
    child: u32,
}

impl KqueueExitWatch {
    const PEEK_FLAGS: libc::c_int = libc::WEXITED | libc::WNOHANG | libc::WNOWAIT;

    pub fn block_until_exit(child: &Child, deadline: Instant) -> io::Result<()> {
        Self::watching(child.id())?.wait_for_exit(deadline)
    }

    fn watching(child: u32) -> io::Result<Self> {
        Self::new_queue().map(|queue| Self { queue, child })
    }

    fn new_queue() -> io::Result<OwnedFd> {
        // SAFETY: kqueue takes no arguments; only a descriptor it returned reaches
        // from_raw_fd, and the OwnedFd is that descriptor's only owner.
        unsafe { Errno::result(libc::kqueue()).map(|raw| OwnedFd::from_raw_fd(raw)) }
    }

    fn exit_note(&self) -> libc::kevent {
        libc::kevent {
            ident: self.child as libc::uintptr_t,
            filter: libc::EVFILT_PROC,
            flags: libc::EV_ADD,
            fflags: libc::NOTE_EXIT,
            data: 0,
            udata: ptr::null_mut(),
        }
    }

    fn timespec(timeout: Duration) -> libc::timespec {
        libc::timespec {
            tv_sec: libc::time_t::try_from(timeout.as_secs()).unwrap_or(libc::time_t::MAX),
            tv_nsec: libc::c_long::from(timeout.subsec_nanos()),
        }
    }
}

impl ExitQueue for KqueueExitWatch {
    fn child(&self) -> u32 {
        self.child
    }

    fn register(&self) -> io::Result<()> {
        let change = self.exit_note();
        // SAFETY: kevent reads the one change on this stack frame and writes no event,
        // since the event list is empty.
        let registered = unsafe {
            libc::kevent(
                self.queue.as_raw_fd(),
                &change,
                1,
                ptr::null_mut(),
                0,
                ptr::null(),
            )
        };
        Errno::result(registered).map(drop)
    }

    fn peek(&self) -> io::Result<WaitReport> {
        let mut info = libc::siginfo_t::default();
        // SAFETY: waitid writes only the siginfo_t on this stack frame; WNOWAIT leaves the
        // child unreaped, so its pid keeps naming it for stop.
        let peeked = unsafe { libc::waitid(libc::P_PID, self.child, &mut info, Self::PEEK_FLAGS) };
        Errno::result(peeked).map(|_| WaitReport::new(info.si_pid, info.si_code))
    }

    fn block_for(&self, timeout: Duration) -> io::Result<()> {
        let mut event = libc::kevent::default();
        let limit = Self::timespec(timeout);
        // SAFETY: kevent reads the timeout on this stack frame and writes at most the one
        // event on it.
        let waited = unsafe {
            libc::kevent(
                self.queue.as_raw_fd(),
                ptr::null(),
                0,
                &mut event,
                1,
                &limit,
            )
        };
        Errno::result(waited).map(drop)
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    struct WatchFixture;

    impl WatchFixture {
        fn shell(script: &str) -> Child {
            Command::new("sh").args(["-c", script]).spawn().unwrap()
        }
    }

    #[test]
    fn an_exited_child_is_seen_through_its_kqueue() {
        let mut child = WatchFixture::shell("exit 0");

        let waited =
            KqueueExitWatch::block_until_exit(&child, Instant::now() + Duration::from_secs(5));

        assert!(waited.is_ok());
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    fn a_running_child_is_waited_for_until_the_deadline() {
        let mut child = WatchFixture::shell("exec sleep 5");

        let waited =
            KqueueExitWatch::block_until_exit(&child, Instant::now() + Duration::from_millis(200));
        let running = child.try_wait().unwrap().is_none();
        let _ = child.kill();
        let _ = child.wait();

        assert!(waited.is_ok());
        assert!(running);
    }
}
