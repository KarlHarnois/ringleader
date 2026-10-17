use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::process::Child;
use std::time::Instant;

pub struct PidfdExitWatch;

impl PidfdExitWatch {
    const MICROS_PER_MILLI: u128 = 1000;

    pub fn block_until_exit(child: &Child, deadline: Instant) -> io::Result<()> {
        let pid = libc::pid_t::try_from(child.id()).map_err(io::Error::other)?;
        // SAFETY: pidfd_open creates a descriptor that only OwnedFd closes, for a child this
        // thread has not reaped, so the pid still names it; poll reads and writes only the one
        // pollfd on this stack frame, which outlives the call.
        unsafe {
            let raw = libc::syscall(libc::SYS_pidfd_open, pid, 0);
            if raw < 0 {
                return Err(io::Error::last_os_error());
            }
            let exit = OwnedFd::from_raw_fd(RawFd::try_from(raw).map_err(io::Error::other)?);
            let mut readable = libc::pollfd {
                fd: exit.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            while libc::poll(&mut readable, 1, Self::milliseconds_until(deadline)) < 0 {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    fn milliseconds_until(deadline: Instant) -> libc::c_int {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let rounded_up = remaining.as_micros().div_ceil(Self::MICROS_PER_MILLI);
        libc::c_int::try_from(rounded_up).unwrap_or(libc::c_int::MAX)
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::time::Duration;

    use super::*;

    struct WatchFixture;

    impl WatchFixture {
        fn shell(script: &str) -> Child {
            Command::new("sh").args(["-c", script]).spawn().unwrap()
        }
    }

    #[test]
    fn an_exited_child_is_seen_through_its_pidfd() {
        let mut child = WatchFixture::shell("exit 0");

        let waited =
            PidfdExitWatch::block_until_exit(&child, Instant::now() + Duration::from_secs(5));

        assert!(waited.is_ok());
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    fn a_running_child_is_waited_for_until_the_deadline() {
        let mut child = WatchFixture::shell("exec sleep 5");

        let waited =
            PidfdExitWatch::block_until_exit(&child, Instant::now() + Duration::from_millis(200));
        let running = child.try_wait().unwrap().is_none();
        let _ = child.kill();
        let _ = child.wait();

        assert!(waited.is_ok());
        assert!(running);
    }
}
